//! Task-generic semantic-intent-recovery proposal boundary.

use std::{
    collections::{BTreeMap, HashSet},
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use cairn_agent::{
    AgentEpisode, AgentStep, AgentStepState, CanonicalToolResult, ContextBlock, DispatchCompletion,
    EpisodeAdvance, EpisodeBudget, EpisodeCompletionReason, EpisodeOperationAdmissionOutcome,
    HistoryItem, InstructionBlock, ModelName, ModelOutputTokenLimit, ModelSelection,
    ModelTransport, NativeProtocolCodec, NativeRequestSpec, NativeToolDefinition, OperationResult,
    PolicyDocument, PreparedToolOperation, ResolvedRuntimeModelArtifact, SettledAgentStep,
    StepOperationSettlement, ToolCatalog, ToolEffectClass, ToolGateway, ToolGatewayError,
    ToolImplementationVersion, ToolName, ToolOperationAssignment, ToolRegistration,
    TurnInputDecision, admit_episode_operations, advance_agent_episode, authorize_tool_operation,
    begin_model_dispatch, begin_tool_operation, execute_model_dispatch, execute_tool_operation,
    open_agent_episode, prepare_native_episode_step, recover_agent_step, settle_decoded_step,
    settle_step_operations,
};
use cairn_protocol::{
    AttemptId, CommandId, ContentId, ContentType, EpisodeId, ModelAttemptId, ObservedAtUnixMillis,
    OperationId, StepId, TaskId,
};
use cairn_record::{ContentStore, ContentStoreError, EventStore};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};
use thiserror::Error;

const SCHEMA_V1: u16 = 1;
const READ_TOOL: &str = "sir_read_task_artifact";
const SUBMIT_TOOL: &str = "sir_submit_intent_hypotheses";
const TOOL_VERSION: &str = "sir-proposal-v1";
const SIR_USER_REQUEST_V1: &str =
    "Inspect the offered task artifacts and submit one cited intent-hypothesis proposal.";
const MAX_HYPOTHESES: usize = 8;
const MAX_UNKNOWNS: usize = 16;
const MAX_FACTS_PER_HYPOTHESIS: usize = 16;
const MAX_CITATIONS_PER_FACT: usize = 8;

const SIR_INSTRUCTION_V1: &str = r"You are the semantic-intent-recovery analyst for one CUDA-to-Ascend-C migration task.

Inspect only the offered task artifacts. First use sir_read_task_artifact to read the source, host launch, ABI, tests, or build files needed for your analysis. Treat observable source facts separately from intent inferences. Cite exact task-local paths and inclusive line ranges.

Submit exactly one complete proposal through sir_submit_intent_hypotheses. It must contain at least two genuinely competing intent hypotheses and at least one unresolved question. Each hypothesis must cite supporting source facts; include counter-facts when the task artifacts challenge it. Preserve uncertainty instead of inventing caller requirements, deployment policy, numerical allowances, or hidden test expectations.

The proposal is non-authoritative. Do not claim admission, correctness, a confidence score, or a migration verdict. Do not invent content identities or use paths outside the offered task bundle.";

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

/// Failure while driving the product SIR workflow through the domain-neutral agent runtime.
#[derive(Debug, Error)]
pub enum SirEpisodeRunError {
    /// Product input, proposal, or task adapter failed.
    #[error(transparent)]
    Sir(#[from] SirError),
    /// The durable agent runtime rejected or could not reconstruct a transition.
    #[error("SIR agent episode failed: {0}")]
    Agent(String),
    /// The model requested a capability outside the fixed DEV-004 profile.
    #[error("SIR model requested an unavailable tool: {0}")]
    UnavailableTool(String),
    /// The episode terminated without one accepted proposal.
    #[error("SIR episode terminated without a proposal: {0:?}")]
    MissingProposal(EpisodeCompletionReason),
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

macro_rules! bounded_text_type {
    ($(#[$meta:meta])* $name:ident, $kind:literal, $max:expr) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates bounded, trimmed, single-line semantic text.
            ///
            /// # Errors
            ///
            /// Rejects empty, oversized, surrounding-whitespace, or control-containing values.
            pub fn new(value: impl Into<String>) -> Result<Self, SirError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > $max
                    || value.trim() != value
                    || value.chars().any(char::is_control)
                {
                    return Err(SirError::InvalidValue($kind));
                }
                Ok(Self(value))
            }

            /// Returns the validated text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

bounded_text_type!(
    /// One source-grounded observation statement.
    SirFactStatement,
    "fact statement",
    2_000
);
bounded_text_type!(
    /// One proposed intent interpretation.
    SirHypothesisSummary,
    "hypothesis summary",
    2_000
);
bounded_text_type!(
    /// One unresolved intent question.
    SirUnknownQuestion,
    "unknown question",
    2_000
);

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
#[derive(Clone)]
pub struct SirTaskWorkspace {
    bundle: SirTaskBundleV1,
    sources: BTreeMap<SirTaskArtifactPath, String>,
}

impl SirTaskWorkspace {
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

    fn source(&self, path: &SirTaskArtifactPath) -> Option<&str> {
        self.sources.get(path).map(String::as_str)
    }

    fn artifact(&self, path: &SirTaskArtifactPath) -> Option<&SirTaskArtifactV1> {
        self.bundle
            .artifacts
            .binary_search_by(|artifact| artifact.path.cmp(path))
            .ok()
            .map(|index| &self.bundle.artifacts[index])
    }

    fn validate_citation(&self, citation: &SirSourceCitationV1) -> Result<(), SirError> {
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

/// Source-grounded fact embedded directly in a hypothesis.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirCitedFactV1 {
    statement: SirFactStatement,
    citations: Vec<SirSourceCitationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirCitedFactWire {
    statement: SirFactStatement,
    citations: Vec<SirSourceCitationV1>,
}

impl TryFrom<SirCitedFactWire> for SirCitedFactV1 {
    type Error = SirError;

    fn try_from(wire: SirCitedFactWire) -> Result<Self, Self::Error> {
        let fact = Self {
            statement: wire.statement,
            citations: wire.citations,
        };
        fact.validate()?;
        Ok(fact)
    }
}

impl<'de> Deserialize<'de> for SirCitedFactV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirCitedFactWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirCitedFactV1 {
    fn validate(&self) -> Result<(), SirError> {
        if self.citations.is_empty() || self.citations.len() > MAX_CITATIONS_PER_FACT {
            return Err(SirError::InvalidStructure("fact citation count"));
        }
        Ok(())
    }
}

/// One plausible intent interpretation and its directly embedded evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirIntentHypothesisV1 {
    summary: SirHypothesisSummary,
    supporting_facts: Vec<SirCitedFactV1>,
    counter_facts: Vec<SirCitedFactV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirIntentHypothesisWire {
    summary: SirHypothesisSummary,
    supporting_facts: Vec<SirCitedFactV1>,
    counter_facts: Vec<SirCitedFactV1>,
}

impl TryFrom<SirIntentHypothesisWire> for SirIntentHypothesisV1 {
    type Error = SirError;

    fn try_from(wire: SirIntentHypothesisWire) -> Result<Self, Self::Error> {
        let hypothesis = Self {
            summary: wire.summary,
            supporting_facts: wire.supporting_facts,
            counter_facts: wire.counter_facts,
        };
        hypothesis.validate()?;
        Ok(hypothesis)
    }
}

impl<'de> Deserialize<'de> for SirIntentHypothesisV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirIntentHypothesisWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirIntentHypothesisV1 {
    fn validate(&self) -> Result<(), SirError> {
        if self.supporting_facts.is_empty()
            || self.supporting_facts.len() > MAX_FACTS_PER_HYPOTHESIS
            || self.counter_facts.len() > MAX_FACTS_PER_HYPOTHESIS
        {
            return Err(SirError::InvalidStructure("hypothesis fact count"));
        }
        for fact in self.supporting_facts.iter().chain(&self.counter_facts) {
            fact.validate()?;
        }
        Ok(())
    }
}

/// One unresolved question retained by the proposal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirUnknownV1 {
    question: SirUnknownQuestion,
    citations: Vec<SirSourceCitationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirUnknownWire {
    question: SirUnknownQuestion,
    citations: Vec<SirSourceCitationV1>,
}

impl TryFrom<SirUnknownWire> for SirUnknownV1 {
    type Error = SirError;

    fn try_from(wire: SirUnknownWire) -> Result<Self, Self::Error> {
        let unknown = Self {
            question: wire.question,
            citations: wire.citations,
        };
        unknown.validate()?;
        Ok(unknown)
    }
}

impl<'de> Deserialize<'de> for SirUnknownV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirUnknownWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirUnknownV1 {
    fn validate(&self) -> Result<(), SirError> {
        if self.citations.len() > MAX_CITATIONS_PER_FACT {
            return Err(SirError::InvalidStructure("unknown citation count"));
        }
        Ok(())
    }
}

/// Complete model-authored body accepted only as a proposal.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SirProposalSubmissionV1 {
    schema_version: u16,
    hypotheses: Vec<SirIntentHypothesisV1>,
    unknowns: Vec<SirUnknownV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SirProposalSubmissionWire {
    schema_version: u16,
    hypotheses: Vec<SirIntentHypothesisV1>,
    unknowns: Vec<SirUnknownV1>,
}

impl TryFrom<SirProposalSubmissionWire> for SirProposalSubmissionV1 {
    type Error = SirError;

    fn try_from(wire: SirProposalSubmissionWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("proposal schema"));
        }
        if !(2..=MAX_HYPOTHESES).contains(&wire.hypotheses.len()) {
            return Err(SirError::InvalidStructure("hypothesis count"));
        }
        if wire.unknowns.is_empty() || wire.unknowns.len() > MAX_UNKNOWNS {
            return Err(SirError::InvalidStructure("unknown count"));
        }
        let mut summaries = HashSet::new();
        for hypothesis in &wire.hypotheses {
            hypothesis.validate()?;
            if !summaries.insert(hypothesis.summary.as_str()) {
                return Err(SirError::InvalidStructure(
                    "hypothesis summaries must be unique",
                ));
            }
        }
        let mut questions = HashSet::new();
        for unknown in &wire.unknowns {
            unknown.validate()?;
            if !questions.insert(unknown.question.as_str()) {
                return Err(SirError::InvalidStructure(
                    "unknown questions must be unique",
                ));
            }
        }
        Ok(Self {
            schema_version: wire.schema_version,
            hypotheses: wire.hypotheses,
            unknowns: wire.unknowns,
        })
    }
}

impl<'de> Deserialize<'de> for SirProposalSubmissionV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        SirProposalSubmissionWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl SirProposalSubmissionV1 {
    /// Returns the competing hypotheses.
    #[must_use]
    pub fn hypotheses(&self) -> &[SirIntentHypothesisV1] {
        &self.hypotheses
    }

    /// Returns the explicit unknowns.
    #[must_use]
    pub fn unknowns(&self) -> &[SirUnknownV1] {
        &self.unknowns
    }

    fn validate_against(&self, workspace: &SirTaskWorkspace) -> Result<(), SirError> {
        for citation in self
            .hypotheses
            .iter()
            .flat_map(|hypothesis| {
                hypothesis
                    .supporting_facts
                    .iter()
                    .chain(&hypothesis.counter_facts)
            })
            .flat_map(|fact| &fact.citations)
            .chain(self.unknowns.iter().flat_map(|unknown| &unknown.citations))
        {
            workspace.validate_citation(citation)?;
        }
        Ok(())
    }
}

/// Trusted provenance envelope around one model-authored hypothesis set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IntentHypothesisSetProposalV1 {
    schema_version: u16,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    submission: SirProposalSubmissionV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IntentHypothesisSetProposalWire {
    schema_version: u16,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    submission: SirProposalSubmissionV1,
}

impl TryFrom<IntentHypothesisSetProposalWire> for IntentHypothesisSetProposalV1 {
    type Error = SirError;

    fn try_from(wire: IntentHypothesisSetProposalWire) -> Result<Self, Self::Error> {
        if wire.schema_version != SCHEMA_V1 {
            return Err(SirError::InvalidStructure("proposal envelope schema"));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            task_bundle: wire.task_bundle,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            submission: wire.submission,
        })
    }
}

impl<'de> Deserialize<'de> for IntentHypothesisSetProposalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        IntentHypothesisSetProposalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl IntentHypothesisSetProposalV1 {
    fn new(
        task_bundle: ContentId<SirTaskBundleArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
        submission: SirProposalSubmissionV1,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            task_bundle,
            episode_id,
            model_configuration,
            submission,
        }
    }

    /// Returns the exact task bundle interpreted by the proposal.
    #[must_use]
    pub const fn task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.task_bundle
    }

    /// Returns the model-authored, non-authoritative body.
    #[must_use]
    pub const fn submission(&self) -> &SirProposalSubmissionV1 {
        &self.submission
    }

    /// Derives the exact proposal identity.
    ///
    /// # Errors
    ///
    /// Returns an error only if canonical encoding fails.
    pub fn identity(&self) -> Result<ContentId<SirIntentHypothesisSetProposalArtifact>, SirError> {
        ContentId::derive(&encode(self)?).map_err(|error| SirError::Codec(error.to_string()))
    }
}

/// Exact archived model-input projection for a SIR episode.
#[derive(Clone, Debug)]
struct SirPromptProjectionV1 {
    task_bundle: ContentId<SirTaskBundleArtifact>,
    instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    request: ContentId<HistoryItem>,
    context: ContentId<ContextBlock>,
    policy: ContentId<PolicyDocument>,
    user_text: String,
}

impl SirPromptProjectionV1 {
    /// Returns the exact task bundle identity.
    #[must_use]
    const fn task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.task_bundle
    }

    /// Returns the exact initial provider-visible request containing only the task manifest.
    #[must_use]
    fn user_text(&self) -> &str {
        &self.user_text
    }

    /// Constructs the generic audited decision for one episode step.
    #[must_use]
    fn turn_input_decision(
        &self,
        selection: ModelSelection,
        pending_results: Vec<ContentId<OperationResult>>,
    ) -> TurnInputDecision {
        TurnInputDecision {
            selection,
            instructions: vec![self.instruction],
            tool_catalog: self.tool_catalog,
            history: vec![self.request],
            context: vec![self.context],
            pending_results,
            policy: self.policy,
        }
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
    let manifest = json!({
        "schema_version":SCHEMA_V1,
        "task_bundle":task_bundle,
        "artifacts":workspace.bundle().artifacts()
    });
    let manifest_text = String::from_utf8(encode(&manifest)?)
        .map_err(|_| SirError::Codec("task manifest is not UTF-8".to_owned()))?;
    let user_text = format!("{SIR_USER_REQUEST_V1}\n\nTask manifest:\n{manifest_text}");
    let request = put_json::<HistoryItem>(store, &json!({"role":"user","content":user_text}))?;
    let context = put_json::<ContextBlock>(store, &manifest)?;
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
        task_bundle,
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
        let artifact = self
            .workspace
            .artifact(&request.path)
            .ok_or_else(|| ToolGatewayError::Rejected("task artifact is not offered".to_owned()))?;
        if request.start_line.get() > artifact.line_count.get() {
            return rejected("task-artifact start line is outside the file");
        }
        let source = self.workspace.source(&request.path).ok_or_else(|| {
            ToolGatewayError::Rejected("task artifact bytes are unavailable".to_owned())
        })?;
        let start = usize::try_from(request.start_line.get() - 1)
            .map_err(|_| ToolGatewayError::Rejected("task-artifact line overflow".to_owned()))?;
        let requested = usize::try_from(request.line_count.get())
            .map_err(|_| ToolGatewayError::Rejected("task-artifact line overflow".to_owned()))?;
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
                .ok_or_else(|| ToolGatewayError::Rejected("task-artifact byte overflow".to_owned()))
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
    task_bundle: ContentId<SirTaskBundleArtifact>,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
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
        task_bundle: ContentId<SirTaskBundleArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    ) -> Self {
        Self {
            workspace,
            task_bundle,
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
            .validate_against(&self.workspace)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let proposal = IntentHypothesisSetProposalV1::new(
            self.task_bundle,
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

/// Trusted inputs selected before opening one SIR episode.
pub struct SirEpisodeRunInput {
    /// Product task lifecycle identity; the exact source bytes are bound separately by the bundle.
    pub task_id: TaskId,
    /// Durable agent episode identity.
    pub episode_id: EpisodeId,
    /// Exact resolved runtime-model configuration identity.
    pub model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
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
pub struct SirEpisodeRunOutcome {
    episode_id: EpisodeId,
    task_bundle: ContentId<SirTaskBundleArtifact>,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: IntentHypothesisSetProposalV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

impl SirEpisodeRunOutcome {
    /// Returns the durable episode identity.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the exact model-visible task bundle.
    #[must_use]
    pub const fn task_bundle(&self) -> ContentId<SirTaskBundleArtifact> {
        self.task_bundle
    }

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

/// Runs the fixed DEV-004 SIR tool loop through the existing durable agent runtime.
///
/// Recorded and live providers implement the same [`ModelTransport`] seam. All source reads and
/// proposal submissions pass through durable operation admission and execution; the model never
/// receives direct filesystem access or proposal authority.
///
/// # Errors
///
/// Returns a product-boundary, durable runtime, transport, tool, budget, or missing-proposal
/// failure. A provider/tool ambiguity remains durable and is never retried implicitly.
#[allow(clippy::too_many_lines)]
pub fn run_sir_episode<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    codec: NativeProtocolCodec,
    workspace: SirTaskWorkspace,
    input: SirEpisodeRunInput,
) -> Result<SirEpisodeRunOutcome, SirEpisodeRunError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let projection = archive_sir_prompt(content, &workspace)?;
    let spec =
        SirPromptProjectionV1::native_spec(input.selection.model.clone(), input.max_output_tokens)
            .map_err(SirEpisodeRunError::Sir)?;
    let episode = AgentEpisode::new(input.episode_id)
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
    let first_step_id = StepId::new();
    let first_attempt_id = ModelAttemptId::new();
    let mut authority = open_agent_episode(
        events,
        &episode,
        input.task_id,
        cairn_agent::AgentRoleName::new("sir-intent-analyst")
            .map_err(|_| SirEpisodeRunError::Agent("invalid built-in SIR role".to_owned()))?,
        input.budget,
        first_step_id,
        first_attempt_id,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
    let mut native = codec
        .prepare_initial(&spec, projection.user_text())
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
    let mut pending_results = Vec::new();
    let mut read_gateway = SirReadTaskArtifactGateway::new(workspace.clone(), input.task_limits);
    let mut submit_gateway = SirSubmitIntentHypothesesGateway::new(
        workspace,
        projection.task_bundle(),
        input.episode_id,
        input.model_configuration,
    );

    loop {
        let step_id = authority.step_id();
        let attempt_id = authority.model_attempt_id();
        let decision =
            projection.turn_input_decision(input.selection.clone(), pending_results.clone());
        let dispatch = prepare_native_episode_step(
            events,
            content,
            authority,
            &decision,
            &native,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        let started = begin_model_dispatch(events, dispatch, &CommandId::new(), observed_now()?)
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        match execute_model_dispatch(
            events,
            content,
            transport,
            started,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        {
            DispatchCompletion::Response(_) => {}
            DispatchCompletion::NotSent { diagnostic }
            | DispatchCompletion::Rejected { diagnostic }
            | DispatchCompletion::Ambiguous { diagnostic } => {
                return Err(SirEpisodeRunError::Agent(diagnostic));
            }
        }

        let step = AgentStep::new(step_id)
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        let AgentStepState::ReadyToDecode(received) =
            recover_agent_step(events, content, &step, attempt_id)
                .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        else {
            return Err(SirEpisodeRunError::Agent(
                "model response did not recover at the decode boundary".to_owned(),
            ));
        };
        let decoded = codec
            .decode_recovered_received(
                events,
                content,
                received,
                &CommandId::new(),
                observed_now()?,
            )
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        let continuation = decoded.continuation().clone();
        let proposed_tools = decoded
            .semantic()
            .proposals()
            .iter()
            .map(|proposal| proposal.tool().as_str().to_owned())
            .collect::<Vec<_>>();
        let settled = settle_decoded_step(
            events,
            content,
            &step,
            attempt_id,
            decoded.into_semantic(),
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;

        match settled {
            SettledAgentStep::Yielded { .. } => {
                let EpisodeAdvance::Completed {
                    reason,
                    steps_started,
                } = advance_agent_episode(
                    events,
                    content,
                    &episode,
                    StepId::new(),
                    ModelAttemptId::new(),
                    &CommandId::new(),
                    observed_now()?,
                )
                .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
                else {
                    return Err(SirEpisodeRunError::Agent(
                        "yielded step unexpectedly advanced".to_owned(),
                    ));
                };
                return finish_sir_episode(
                    content,
                    &projection,
                    &submit_gateway,
                    reason,
                    steps_started,
                );
            }
            SettledAgentStep::AwaitingOperations { .. } => {}
        }

        let registrations = sir_tool_registrations()?;
        let assignments = proposed_tools
            .iter()
            .map(|name| {
                let registration = registrations
                    .iter()
                    .find(|registration| registration.name().as_str() == name)
                    .cloned()
                    .ok_or_else(|| SirEpisodeRunError::UnavailableTool(name.clone()))?;
                Ok(ToolOperationAssignment::new(
                    OperationId::new(),
                    registration,
                ))
            })
            .collect::<Result<Vec<_>, SirEpisodeRunError>>()?;
        let admission = match admit_episode_operations(
            events,
            content,
            &episode,
            assignments,
            &CommandId::new(),
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        {
            EpisodeOperationAdmissionOutcome::Admitted(admission) => admission,
            EpisodeOperationAdmissionOutcome::Completed {
                reason,
                steps_started,
            } => {
                return finish_sir_episode(
                    content,
                    &projection,
                    &submit_gateway,
                    reason,
                    steps_started,
                );
            }
        };

        for operation in admission.into_operations() {
            let tool = operation.tool().as_str().to_owned();
            let operation_authority =
                authorize_tool_operation(events, &CommandId::new(), observed_now()?, operation)
                    .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
            let started = begin_tool_operation(
                events,
                operation_authority,
                AttemptId::new(),
                &CommandId::new(),
                observed_now()?,
            )
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
            if tool == READ_TOOL {
                let _ = execute_tool_operation(
                    events,
                    content,
                    &mut read_gateway,
                    started,
                    &CommandId::new(),
                    observed_now()?,
                )
                .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
            } else if tool == SUBMIT_TOOL {
                let _ = execute_tool_operation(
                    events,
                    content,
                    &mut submit_gateway,
                    started,
                    &CommandId::new(),
                    observed_now()?,
                )
                .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
            } else {
                return Err(SirEpisodeRunError::UnavailableTool(tool));
            }
        }

        let StepOperationSettlement::ReadyForNextStep {
            pending_results: results,
            ..
        } = settle_step_operations(
            events,
            content,
            &step,
            attempt_id,
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        else {
            return Err(SirEpisodeRunError::Agent(
                "SIR tool operation requires explicit reconciliation".to_owned(),
            ));
        };
        let settled_continuation = codec
            .append_archived_tool_results(content, &continuation, &results)
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        native = codec
            .prepare_continuation(&spec, &settled_continuation)
            .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?;
        pending_results = results;

        match advance_agent_episode(
            events,
            content,
            &episode,
            StepId::new(),
            ModelAttemptId::new(),
            &CommandId::new(),
            observed_now()?,
        )
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        {
            EpisodeAdvance::NextStep(next) => authority = next,
            EpisodeAdvance::Completed {
                reason,
                steps_started,
            } => {
                return finish_sir_episode(
                    content,
                    &projection,
                    &submit_gateway,
                    reason,
                    steps_started,
                );
            }
        }
    }
}

fn finish_sir_episode<C: ContentStore>(
    content: &mut C,
    projection: &SirPromptProjectionV1,
    submit_gateway: &SirSubmitIntentHypothesesGateway,
    reason: EpisodeCompletionReason,
    steps_started: u32,
) -> Result<SirEpisodeRunOutcome, SirEpisodeRunError> {
    let Some((proposal_id, proposal)) = submit_gateway.accepted().cloned() else {
        return Err(SirEpisodeRunError::MissingProposal(reason));
    };
    let archived = content
        .put::<SirIntentHypothesisSetProposalArtifact>(&mut Cursor::new(encode(&proposal)?))
        .map_err(SirError::Content)?
        .content_id;
    if archived != proposal_id {
        return Err(SirEpisodeRunError::Agent(
            "archived SIR proposal identity changed".to_owned(),
        ));
    }
    Ok(SirEpisodeRunOutcome {
        episode_id: proposal.episode_id,
        task_bundle: projection.task_bundle(),
        proposal_id,
        proposal,
        completion_reason: reason,
        steps_started,
    })
}

/// Returns trusted registrations for the two DEV-004 tools.
///
/// # Errors
///
/// Returns an error only if repository-owned labels violate generic agent contracts.
fn sir_tool_registrations() -> Result<[ToolRegistration; 2], SirError> {
    Ok([
        ToolRegistration::new(
            ToolName::new(READ_TOOL).map_err(|_| SirError::InvalidValue("built-in tool name"))?,
            ToolImplementationVersion::new(TOOL_VERSION)
                .map_err(|_| SirError::InvalidValue("built-in tool version"))?,
            ToolEffectClass::ReadOnly,
        ),
        ToolRegistration::new(
            ToolName::new(SUBMIT_TOOL).map_err(|_| SirError::InvalidValue("built-in tool name"))?,
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
fn sir_native_tools() -> Result<Vec<NativeToolDefinition>, SirError> {
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
    let fact = json!({
        "type":"object",
        "properties":{
            "statement":{"type":"string","minLength":1,"maxLength":2000},
            "citations":{"type":"array","minItems":1,"maxItems":MAX_CITATIONS_PER_FACT,"items":citation}
        },
        "required":["statement","citations"],
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
                    "hypotheses":{
                        "type":"array","minItems":2,"maxItems":MAX_HYPOTHESES,
                        "items":{
                            "type":"object",
                            "properties":{
                                "summary":{"type":"string","minLength":1,"maxLength":2000},
                                "supporting_facts":{"type":"array","minItems":1,"maxItems":MAX_FACTS_PER_HYPOTHESIS,"items":fact},
                                "counter_facts":{"type":"array","maxItems":MAX_FACTS_PER_HYPOTHESIS,"items":fact}
                            },
                            "required":["summary","supporting_facts","counter_facts"],
                            "additionalProperties":false
                        }
                    },
                    "unknowns":{
                        "type":"array","minItems":1,"maxItems":MAX_UNKNOWNS,
                        "items":{
                            "type":"object",
                            "properties":{
                                "question":{"type":"string","minLength":1,"maxLength":2000},
                                "citations":{"type":"array","maxItems":MAX_CITATIONS_PER_FACT,"items":citation}
                            },
                            "required":["question","citations"],
                            "additionalProperties":false
                        }
                    }
                },
                "required":["schema_version","hypotheses","unknowns"],
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

fn encode(value: &impl Serialize) -> Result<Vec<u8>, SirError> {
    cairn_codec::to_vec(value).map_err(|error| SirError::Codec(error.to_string()))
}

fn observed_now() -> Result<ObservedAtUnixMillis, SirEpisodeRunError> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| SirEpisodeRunError::Agent(error.to_string()))?
        .as_millis();
    let millis = i64::try_from(millis)
        .map_err(|_| SirEpisodeRunError::Agent("wall clock overflow".to_owned()))?;
    Ok(ObservedAtUnixMillis::new(millis))
}

fn put_json<T: ContentType>(
    store: &mut impl ContentStore,
    value: &Value,
) -> Result<ContentId<T>, SirError> {
    let bytes = encode(value)?;
    Ok(store.put::<T>(&mut Cursor::new(bytes))?.content_id)
}
