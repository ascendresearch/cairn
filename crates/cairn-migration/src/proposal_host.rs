//! Generic Proposal Host boundary over domain-specific SIR and Candidate role profiles.

use cairn_agent::{
    EpisodeBudget, EpisodeCompletionReason, ModelOutputTokenLimit, ModelSelection, ModelTransport,
    NativeProtocolCodec, ToolArguments, ToolEffectClass, ToolImplementationVersion, ToolName,
};
use cairn_protocol::{
    ContentId, ContentType, EpisodeId, ModelAttemptId, OperationId, SchemaVersion, StepId, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, CandidateOracleContractV1, CandidateOracleMaterialsV1,
    CandidateProposalArtifact, CandidateProposalV1, CandidateStrategyProfileInput,
    CandidateWorkspaceV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, OracleBuildTestSnapshotArtifact, OracleClaimV1,
    OracleDocumentationSnapshotArtifact, OracleKnowledgeSnapshotArtifact, OracleStrategyExecutorV1,
    OracleStrategyProfileInput, OracleStrategyRunV1, OracleStrategySubmissionArtifact,
    OracleStrategySubmissionV1, OracleStrategyToolCatalogV1, OracleWorkItemV1, OracleWorkspaceV1,
    SirCapabilityManifestV1, SirIntentHypothesisSetProposalArtifact, SirProfileInput,
    SirTaskArtifactPath, SirTaskBundleV1, SirTaskLimits, SirTaskWorkspace,
    run_candidate_strategy_profile, run_oracle_strategy_profile, run_sir_profile,
};

/// Exact complete runtime invocation snapshot archived before an episode starts.
pub enum ProposalHostInvocationArtifact {}
impl ContentType for ProposalHostInvocationArtifact {
    const DOMAIN: &'static str = "migration.proposal-host-invocation.v1";
}

/// Canonical request accepted by one generic Proposal Host episode.
pub enum ProposalHostRequestArtifact {}

impl ContentType for ProposalHostRequestArtifact {
    const DOMAIN: &'static str = "migration.proposal-host-request.v1";
}

/// Canonical terminal outcome returned by one generic Proposal Host episode.
pub enum ProposalHostTerminalArtifact {}

impl ContentType for ProposalHostTerminalArtifact {
    const DOMAIN: &'static str = "migration.proposal-host-terminal.v1";
}

/// Canonical durable-yield identity returned before Controller-owned experiment execution.
///
/// ```compile_fail
/// use cairn_migration::{ProposalHostExperimentRequestArtifact, ProposalHostTerminalArtifact};
/// use cairn_protocol::ContentId;
/// fn require_experiment(_: ContentId<ProposalHostExperimentRequestArtifact>) {}
/// fn invalid(terminal: ContentId<ProposalHostTerminalArtifact>) {
///     require_experiment(terminal);
/// }
/// ```
pub enum ProposalHostExperimentRequestArtifact {}

impl ContentType for ProposalHostExperimentRequestArtifact {
    const DOMAIN: &'static str = "migration.proposal-host-experiment-request.v1";
}

/// Exact digest of the generic Proposal Host executable authorized for an invocation.
///
/// This identity is intentionally distinct from a managed Worker's binary identity: a Proposal
/// Host has model/proposal authority and never receives execution authority.
///
/// ```compile_fail
/// use cairn_execution::WorkerBinaryIdentity;
/// use cairn_migration::ProposalHostBinaryIdentity;
/// fn require_host(_: ProposalHostBinaryIdentity) {}
/// fn invalid(worker: WorkerBinaryIdentity) {
///     require_host(worker);
/// }
/// ```
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProposalHostBinaryIdentity(String);

impl ProposalHostBinaryIdentity {
    /// Creates one exact lowercase SHA-256 executable identity.
    ///
    /// # Errors
    ///
    /// Rejects any value outside the canonical `sha256:<64 lowercase hex>` representation.
    pub fn new(value: impl Into<String>) -> Result<Self, ProposalHostError> {
        let value = value.into();
        let digest = value.strip_prefix("sha256:").ok_or_else(|| {
            ProposalHostError::InvalidRequest(
                "Proposal Host binary identity is not a canonical SHA-256 digest".into(),
            )
        })?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return invalid(
                "Proposal Host binary identity is not a canonical lowercase SHA-256 digest",
            );
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for ProposalHostBinaryIdentity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProposalHostBinaryIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl std::fmt::Display for ProposalHostBinaryIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One exact source entry in a Controller-materialized task snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostTaskSourceV1 {
    path: SirTaskArtifactPath,
    source: String,
}

impl ProposalHostTaskSourceV1 {
    #[must_use]
    pub fn new(path: SirTaskArtifactPath, source: String) -> Self {
        Self { path, source }
    }
}

/// Exact task material projected into a Host without granting an arbitrary filesystem root.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostTaskSnapshotV1 {
    bundle: SirTaskBundleV1,
    sources: Vec<ProposalHostTaskSourceV1>,
}

const ORACLE_HOST_TEXT_SNAPSHOT_LIMIT: usize = 512 * 1024;

macro_rules! oracle_host_text_snapshot {
    ($name:ident, $wire:ident, $artifact:ty, $label:literal) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize)]
        pub struct $name {
            identity: ContentId<$artifact>,
            text: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $wire {
            identity: ContentId<$artifact>,
            text: String,
        }

        impl $name {
            /// Constructs one exact UTF-8 snapshot transferred into the Host process.
            ///
            /// # Errors
            ///
            /// Rejects empty/oversized text or identity drift from the frozen workspace edge.
            pub fn new(
                identity: ContentId<$artifact>,
                text: String,
            ) -> Result<Self, ProposalHostError> {
                if text.is_empty()
                    || text.len() > ORACLE_HOST_TEXT_SNAPSHOT_LIMIT
                    || ContentId::<$artifact>::derive(text.as_bytes()).map_err(codec)? != identity
                {
                    return invalid(concat!($label, " snapshot identity or size changed"));
                }
                Ok(Self { identity, text })
            }

            #[must_use]
            pub fn text(&self) -> &str {
                &self.text
            }

            #[must_use]
            pub const fn identity(&self) -> ContentId<$artifact> {
                self.identity
            }
        }

        impl TryFrom<$wire> for $name {
            type Error = ProposalHostError;

            fn try_from(wire: $wire) -> Result<Self, Self::Error> {
                Self::new(wire.identity, wire.text)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                $wire::deserialize(deserializer)?
                    .try_into()
                    .map_err(de::Error::custom)
            }
        }
    };
}

oracle_host_text_snapshot!(
    ProposalHostOracleDocumentationV1,
    ProposalHostOracleDocumentationWire,
    OracleDocumentationSnapshotArtifact,
    "Oracle documentation"
);
oracle_host_text_snapshot!(
    ProposalHostOracleBuildTestsV1,
    ProposalHostOracleBuildTestsWire,
    OracleBuildTestSnapshotArtifact,
    "Oracle build/test"
);
oracle_host_text_snapshot!(
    ProposalHostOracleKnowledgeV1,
    ProposalHostOracleKnowledgeWire,
    OracleKnowledgeSnapshotArtifact,
    "Oracle knowledge"
);

/// Exact human-readable public material projected into one Oracle strategy Host request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostOracleMaterialsV1 {
    documentation: ProposalHostOracleDocumentationV1,
    build_and_tests: ProposalHostOracleBuildTestsV1,
    knowledge: ProposalHostOracleKnowledgeV1,
}

impl ProposalHostOracleMaterialsV1 {
    #[must_use]
    pub fn new(
        documentation: ProposalHostOracleDocumentationV1,
        build_and_tests: ProposalHostOracleBuildTestsV1,
        knowledge: ProposalHostOracleKnowledgeV1,
    ) -> Self {
        Self {
            documentation,
            build_and_tests,
            knowledge,
        }
    }

    #[must_use]
    pub const fn documentation(&self) -> &ProposalHostOracleDocumentationV1 {
        &self.documentation
    }

    #[must_use]
    pub const fn build_and_tests(&self) -> &ProposalHostOracleBuildTestsV1 {
        &self.build_and_tests
    }

    #[must_use]
    pub const fn knowledge(&self) -> &ProposalHostOracleKnowledgeV1 {
        &self.knowledge
    }
}

impl ProposalHostTaskSnapshotV1 {
    #[must_use]
    pub fn new(bundle: SirTaskBundleV1, sources: Vec<ProposalHostTaskSourceV1>) -> Self {
        Self { bundle, sources }
    }

    /// Copies the exact already-validated workspace into a process-transfer snapshot.
    #[must_use]
    pub fn from_workspace(workspace: &SirTaskWorkspace) -> Self {
        Self {
            bundle: workspace.bundle().clone(),
            sources: workspace
                .materialized_sources()
                .into_iter()
                .map(|(path, source)| ProposalHostTaskSourceV1 { path, source })
                .collect(),
        }
    }

    fn workspace(&self, limits: SirTaskLimits) -> Result<SirTaskWorkspace, ProposalHostError> {
        SirTaskWorkspace::from_materialized(
            self.bundle.clone(),
            self.sources
                .iter()
                .map(|source| (source.path.clone(), source.source.clone()))
                .collect(),
            limits,
        )
        .map_err(|error| ProposalHostError::InvalidRequest(error.to_string()))
    }
}

/// Runtime facts frozen before a Proposal Host may dispatch a model effect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostRuntimeV1 {
    schema_version: SchemaVersion,
    episode_id: EpisodeId,
    binary_identity: ProposalHostBinaryIdentity,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    selection: ModelSelection,
    budget: EpisodeBudget,
    max_output_tokens: ModelOutputTokenLimit,
    task_limits: SirTaskLimits,
}

impl ProposalHostRuntimeV1 {
    #[must_use]
    pub fn new(
        episode_id: EpisodeId,
        binary_identity: ProposalHostBinaryIdentity,
        model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        selection: ModelSelection,
        budget: EpisodeBudget,
        max_output_tokens: ModelOutputTokenLimit,
        task_limits: SirTaskLimits,
    ) -> Self {
        Self {
            schema_version: schema_v1(),
            episode_id,
            binary_identity,
            model_configuration,
            selection,
            budget,
            max_output_tokens,
            task_limits,
        }
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn binary_identity(&self) -> &ProposalHostBinaryIdentity {
        &self.binary_identity
    }

    #[must_use]
    pub const fn model_configuration(&self) -> ContentId<AgentResolvedRuntimeModelArtifact> {
        self.model_configuration
    }

    #[must_use]
    pub const fn selection(&self) -> &ModelSelection {
        &self.selection
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> ModelOutputTokenLimit {
        self.max_output_tokens
    }

    #[must_use]
    pub const fn task_limits(&self) -> SirTaskLimits {
        self.task_limits
    }

    /// Derives the exact invocation snapshot identity persisted by the workflow.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding fails.
    pub fn identity(&self) -> Result<ContentId<ProposalHostInvocationArtifact>, ProposalHostError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal Host runtime is not current V1");
        }
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }
}

/// Closed set of domain-specific role inputs currently consumed by the generic Host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "role")]
#[allow(clippy::large_enum_variant)]
pub enum ProposalHostRoleRequestV1 {
    Sir {
        task_id: TaskId,
        recovery_request: IntentRecoveryRequestV1,
        task: ProposalHostTaskSnapshotV1,
    },
    OracleStrategy {
        workspace: OracleWorkspaceV1,
        claim: OracleClaimV1,
        work_item: OracleWorkItemV1,
        run: OracleStrategyRunV1,
        task: ProposalHostTaskSnapshotV1,
        materials: ProposalHostOracleMaterialsV1,
    },
    CandidateStrategy {
        workspace: CandidateWorkspaceV1,
        contract: CandidateOracleContractV1,
        oracle_materials: CandidateOracleMaterialsV1,
        task: ProposalHostTaskSnapshotV1,
        public_materials: ProposalHostOracleMaterialsV1,
    },
}

/// Exact current-V1 Host request. Deserialization reruns every role binding invariant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostRequestV1 {
    schema_version: SchemaVersion,
    runtime: ProposalHostRuntimeV1,
    role: ProposalHostRoleRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostRequestWire {
    schema_version: SchemaVersion,
    runtime: ProposalHostRuntimeV1,
    role: ProposalHostRoleRequestV1,
}

impl ProposalHostRequestV1 {
    /// Creates and validates one exact role-scoped Host request.
    ///
    /// # Errors
    ///
    /// Rejects task, recovery, parent, diagnostic, workflow, or episode binding drift.
    pub fn new(
        runtime: ProposalHostRuntimeV1,
        role: ProposalHostRoleRequestV1,
    ) -> Result<Self, ProposalHostError> {
        let value = Self {
            schema_version: schema_v1(),
            runtime,
            role,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn runtime(&self) -> &ProposalHostRuntimeV1 {
        &self.runtime
    }

    #[must_use]
    pub const fn role(&self) -> &ProposalHostRoleRequestV1 {
        &self.role
    }

    /// Reconstructs the exact SIR input frozen by this Host request.
    ///
    /// # Errors
    ///
    /// Rejects a non-SIR role or an invalid task/capability binding.
    pub fn sir_recovery_input(&self) -> Result<IntentRecoveryInputV1, ProposalHostError> {
        self.validate()?;
        let ProposalHostRoleRequestV1::Sir {
            task_id,
            recovery_request,
            task,
        } = &self.role
        else {
            return invalid("Proposal Host request is not an SIR role");
        };
        IntentRecoveryInputV1::new(
            *task_id,
            task.bundle.identity().map_err(role_error)?,
            recovery_request.clone(),
            SirCapabilityManifestV1::proposal_only(self.runtime.task_limits),
        )
        .map_err(role_error)
    }

    /// Derives the exact request identity used by the Host terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if validation or canonical encoding fails.
    pub fn identity(&self) -> Result<ContentId<ProposalHostRequestArtifact>, ProposalHostError> {
        self.validate()?;
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }

    #[allow(clippy::too_many_lines)]
    fn validate(&self) -> Result<(), ProposalHostError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal Host request is not current V1");
        }
        let workspace = match &self.role {
            ProposalHostRoleRequestV1::Sir { task, .. }
            | ProposalHostRoleRequestV1::OracleStrategy { task, .. }
            | ProposalHostRoleRequestV1::CandidateStrategy { task, .. } => {
                task.workspace(self.runtime.task_limits)?
            }
        };
        match &self.role {
            ProposalHostRoleRequestV1::Sir { .. } => Ok(()),
            ProposalHostRoleRequestV1::OracleStrategy {
                workspace,
                claim,
                work_item,
                run,
                task,
                materials,
            } => {
                if task.bundle.identity().map_err(role_error)? != workspace.sir_task_bundle()
                    || claim.task_id() != workspace.task_id()
                    || claim.admitted_intent() != workspace.admitted_intent()
                    || work_item.claim() != claim.identity().map_err(role_error)?
                    || run.workspace() != workspace.identity().map_err(role_error)?
                    || run.item() != work_item.identity().map_err(role_error)?
                    || materials.documentation.identity() != workspace.documentation()
                    || materials.build_and_tests.identity() != workspace.build_and_tests()
                    || materials.knowledge.identity() != workspace.knowledge()
                {
                    return invalid(
                        "Oracle strategy request changed its workspace or cell binding",
                    );
                }
                let OracleStrategyExecutorV1::AgentEpisode {
                    invocation, tools, ..
                } = run.executor()
                else {
                    return invalid("Oracle Proposal Host request names a non-Agent executor");
                };
                if *invocation != self.runtime.identity()?
                    || *tools
                        != OracleStrategyToolCatalogV1::standard()
                            .identity()
                            .map_err(role_error)?
                {
                    return invalid(
                        "Oracle strategy run changed its Proposal Host invocation or tool catalog",
                    );
                }
                Ok(())
            }
            ProposalHostRoleRequestV1::CandidateStrategy {
                workspace: candidate_workspace,
                contract,
                oracle_materials,
                public_materials,
                ..
            } => {
                if workspace.bundle().identity().map_err(role_error)?
                    != candidate_workspace.task_bundle()
                    || contract.identity().map_err(role_error)?
                        != candidate_workspace.oracle_contract()
                    || public_materials.documentation().identity()
                        != candidate_workspace.documentation()
                    || public_materials.build_and_tests().identity()
                        != candidate_workspace.build_and_tests()
                    || public_materials.knowledge().identity() != candidate_workspace.knowledge()
                {
                    return invalid(
                        "Candidate strategy request changed its workspace or public material binding",
                    );
                }
                oracle_materials
                    .validate_against(contract)
                    .map_err(role_error)
            }
        }
    }
}

impl TryFrom<ProposalHostRequestWire> for ProposalHostRequestV1 {
    type Error = ProposalHostError;

    fn try_from(wire: ProposalHostRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            runtime: wire.runtime,
            role: wire.role,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalHostRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact external operation proposed by an Agent and durably bound by the Proposal Host.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostExperimentOperationV1 {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
    arguments: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostExperimentOperationWire {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
    arguments: serde_json::Value,
}

impl ProposalHostExperimentOperationV1 {
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    #[must_use]
    pub fn tool(&self) -> &ToolName {
        &self.tool
    }

    #[must_use]
    pub fn implementation_version(&self) -> &ToolImplementationVersion {
        &self.implementation_version
    }

    #[must_use]
    pub const fn effect(&self) -> ToolEffectClass {
        self.effect
    }

    #[must_use]
    pub const fn arguments_id(&self) -> ContentId<ToolArguments> {
        self.arguments_id
    }

    #[must_use]
    pub const fn arguments(&self) -> &serde_json::Value {
        &self.arguments
    }

    fn validate(&self) -> Result<(), ProposalHostError> {
        if matches!(
            self.effect,
            ToolEffectClass::Pure | ToolEffectClass::ReadOnly
        ) {
            return invalid("Proposal Host experiment request contains a Host-local operation");
        }
        let bytes = cairn_codec::to_vec(&self.arguments).map_err(codec)?;
        if ContentId::<ToolArguments>::derive(&bytes).map_err(codec)? != self.arguments_id {
            return invalid("Proposal Host experiment arguments changed their content identity");
        }
        Ok(())
    }
}

impl TryFrom<ProposalHostExperimentOperationWire> for ProposalHostExperimentOperationV1 {
    type Error = ProposalHostError;

    fn try_from(wire: ProposalHostExperimentOperationWire) -> Result<Self, Self::Error> {
        let value = Self {
            operation_id: wire.operation_id,
            tool: wire.tool,
            implementation_version: wire.implementation_version,
            effect: wire.effect,
            arguments_id: wire.arguments_id,
            arguments: wire.arguments,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostExperimentOperationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalHostExperimentOperationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Request-bound durable safe point at which only the Controller may grant experiment authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostExperimentRequestV1 {
    schema_version: SchemaVersion,
    request: ContentId<ProposalHostRequestArtifact>,
    episode_id: EpisodeId,
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    operations: Vec<ProposalHostExperimentOperationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostExperimentRequestWire {
    schema_version: SchemaVersion,
    request: ContentId<ProposalHostRequestArtifact>,
    episode_id: EpisodeId,
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    operations: Vec<ProposalHostExperimentOperationV1>,
}

impl ProposalHostExperimentRequestV1 {
    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalHostRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn step_id(&self) -> StepId {
        self.step_id
    }

    #[must_use]
    pub const fn model_attempt_id(&self) -> ModelAttemptId {
        self.model_attempt_id
    }

    #[must_use]
    pub fn operations(&self) -> &[ProposalHostExperimentOperationV1] {
        &self.operations
    }

    /// Derives the exact durable-yield identity.
    ///
    /// # Errors
    ///
    /// Rejects an invalid structure or canonical codec/content identity failure.
    pub fn identity(
        &self,
    ) -> Result<ContentId<ProposalHostExperimentRequestArtifact>, ProposalHostError> {
        self.validate_structure()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }

    /// Revalidates this yield against the exact Host request that opened the episode.
    ///
    /// # Errors
    ///
    /// Rejects structure, request identity, or episode identity drift.
    pub fn validate_against(
        &self,
        request: &ProposalHostRequestV1,
    ) -> Result<(), ProposalHostError> {
        self.validate_structure()?;
        if self.request != request.identity()? || self.episode_id != request.runtime.episode_id {
            return invalid("Proposal Host experiment changed its request or episode identity");
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), ProposalHostError> {
        if self.schema_version != schema_v1() || self.operations.is_empty() {
            return invalid("Proposal Host experiment request structure is invalid");
        }
        let mut ids = std::collections::HashSet::new();
        for operation in &self.operations {
            operation.validate()?;
            if !ids.insert(operation.operation_id) {
                return invalid("Proposal Host experiment repeats an operation identity");
            }
        }
        Ok(())
    }
}

impl TryFrom<ProposalHostExperimentRequestWire> for ProposalHostExperimentRequestV1 {
    type Error = ProposalHostError;

    fn try_from(wire: ProposalHostExperimentRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            request: wire.request,
            episode_id: wire.episode_id,
            step_id: wire.step_id,
            model_attempt_id: wire.model_attempt_id,
            operations: wire.operations,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostExperimentRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalHostExperimentRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Process result: either a terminal proposal or a durable Controller experiment safe point.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "outcome")]
pub enum ProposalHostOutcomeV1 {
    Terminal {
        terminal: Box<ProposalHostTerminalV1>,
    },
    AwaitingController {
        experiment: ProposalHostExperimentRequestV1,
    },
}

impl ProposalHostOutcomeV1 {
    /// Revalidates either current outcome against the exact Host request.
    ///
    /// # Errors
    ///
    /// Rejects terminal or experiment request/episode/role/binding drift.
    pub fn validate_against(
        &self,
        request: &ProposalHostRequestV1,
    ) -> Result<(), ProposalHostError> {
        match self {
            Self::Terminal { terminal } => terminal.validate_against(request),
            Self::AwaitingController { experiment } => experiment.validate_against(request),
        }
    }
}

/// Typed proposal publication produced by a role-scoped Host episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "role")]
pub enum ProposalHostPublicationV1 {
    Sir {
        proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
        proposal: IntentHypothesisSetProposalV1,
    },
    OracleStrategy {
        submission_id: ContentId<OracleStrategySubmissionArtifact>,
        submission: OracleStrategySubmissionV1,
    },
    CandidateStrategy {
        proposal_id: ContentId<CandidateProposalArtifact>,
        proposal: CandidateProposalV1,
    },
}

/// Exact terminal result bound to one Host request and durable agent episode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostTerminalV1 {
    schema_version: SchemaVersion,
    request: ContentId<ProposalHostRequestArtifact>,
    episode_id: EpisodeId,
    publication: ProposalHostPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostTerminalWire {
    schema_version: SchemaVersion,
    request: ContentId<ProposalHostRequestArtifact>,
    episode_id: EpisodeId,
    publication: ProposalHostPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

impl ProposalHostTerminalV1 {
    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalHostRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn publication(&self) -> &ProposalHostPublicationV1 {
        &self.publication
    }

    #[must_use]
    pub const fn completion_reason(&self) -> EpisodeCompletionReason {
        self.completion_reason
    }

    #[must_use]
    pub const fn steps_started(&self) -> u32 {
        self.steps_started
    }

    /// Derives the exact terminal identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be canonically encoded.
    pub fn identity(&self) -> Result<ContentId<ProposalHostTerminalArtifact>, ProposalHostError> {
        self.validate_structure()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }

    /// Revalidates the terminal against the exact request that authorized the episode.
    ///
    /// # Errors
    ///
    /// Rejects request, role, episode, or publication binding drift.
    pub fn validate_against(
        &self,
        request: &ProposalHostRequestV1,
    ) -> Result<(), ProposalHostError> {
        self.validate_structure()?;
        if self.request != request.identity()? || self.episode_id != request.runtime.episode_id {
            return invalid("Proposal Host terminal changed its request or episode identity");
        }
        let matching_role = matches!(
            (&request.role, &self.publication),
            (
                ProposalHostRoleRequestV1::Sir { .. },
                ProposalHostPublicationV1::Sir { .. }
            ) | (
                ProposalHostRoleRequestV1::OracleStrategy { .. },
                ProposalHostPublicationV1::OracleStrategy { .. }
            ) | (
                ProposalHostRoleRequestV1::CandidateStrategy { .. },
                ProposalHostPublicationV1::CandidateStrategy { .. }
            )
        );
        if !matching_role {
            return invalid("Proposal Host terminal changed its requested role");
        }
        if let (
            ProposalHostRoleRequestV1::OracleStrategy { run, work_item, .. },
            ProposalHostPublicationV1::OracleStrategy { submission, .. },
        ) = (&request.role, &self.publication)
        {
            if submission.run() != run.identity().map_err(role_error)?
                || submission.item() != work_item.identity().map_err(role_error)?
            {
                return invalid("Oracle strategy publication changed its run or work item");
            }
        }
        if let (
            ProposalHostRoleRequestV1::CandidateStrategy { contract, .. },
            ProposalHostPublicationV1::CandidateStrategy { proposal, .. },
        ) = (&request.role, &self.publication)
        {
            if proposal.oracle_contract() != contract.identity().map_err(role_error)?
                || proposal.model_configuration() != request.runtime.model_configuration
            {
                return invalid(
                    "Candidate strategy publication changed its Oracle or model binding",
                );
            }
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), ProposalHostError> {
        if self.schema_version != schema_v1() || self.steps_started == 0 {
            return invalid("Proposal Host terminal structure is invalid");
        }
        let episode_id = match &self.publication {
            ProposalHostPublicationV1::Sir {
                proposal_id,
                proposal,
            } => {
                if proposal.identity().map_err(role_error)? != *proposal_id {
                    return invalid("SIR publication identity changed");
                }
                proposal.episode_id()
            }
            ProposalHostPublicationV1::OracleStrategy {
                submission_id,
                submission,
            } => {
                if submission.identity().map_err(role_error)? != *submission_id {
                    return invalid("Oracle strategy submission identity changed");
                }
                self.episode_id
            }
            ProposalHostPublicationV1::CandidateStrategy {
                proposal_id,
                proposal,
            } => {
                if proposal.identity().map_err(role_error)? != *proposal_id {
                    return invalid("Candidate strategy proposal identity changed");
                }
                proposal.episode_id()
            }
        };
        if episode_id != self.episode_id {
            return invalid("Proposal Host publication changed its episode identity");
        }
        Ok(())
    }
}

impl TryFrom<ProposalHostTerminalWire> for ProposalHostTerminalV1 {
    type Error = ProposalHostError;

    fn try_from(wire: ProposalHostTerminalWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            request: wire.request,
            episode_id: wire.episode_id,
            publication: wire.publication,
            completion_reason: wire.completion_reason,
            steps_started: wire.steps_started,
        };
        value.validate_structure()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostTerminalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalHostTerminalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

struct FrozenProposalHostRequestV1 {
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: ProposalHostRequestV1,
    runtime: ProposalHostRuntimeV1,
    workspace: SirTaskWorkspace,
}

struct CompletedProposalHostRequestV1 {
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: ProposalHostRequestV1,
    episode_id: EpisodeId,
    publication: ProposalHostPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

enum DrivenProposalHostRequestV1 {
    Complete(Box<CompletedProposalHostRequestV1>),
    AwaitingController(Box<AwaitingControllerProposalHostRequestV1>),
}

struct AwaitingControllerProposalHostRequestV1 {
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: ProposalHostRequestV1,
    experiment: crate::proposal_loop::ProposalLoopExperimentRequestV1,
}

/// Processes any supported request through the single frozen-input Proposal Host lifecycle.
///
/// # Errors
///
/// Rejects invalid frozen material, durable Agent Loop failures, invalid typed submissions, or a
/// terminal outcome that does not bind to the exact request.
pub fn run_proposal_host_episode<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    protocol_codec: NativeProtocolCodec,
    request: ProposalHostRequestV1,
) -> Result<ProposalHostOutcomeV1, ProposalHostError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let frozen = freeze_proposal_host_request(request)?;
    let driven =
        drive_frozen_proposal_host_request(events, content, transport, protocol_codec, frozen)?;
    freeze_proposal_host_outcome(driven)
}

fn freeze_proposal_host_request(
    request: ProposalHostRequestV1,
) -> Result<FrozenProposalHostRequestV1, ProposalHostError> {
    request.validate()?;
    let request_id = request.identity()?;
    let runtime = request.runtime.clone();
    let workspace = match &request.role {
        ProposalHostRoleRequestV1::Sir { task, .. }
        | ProposalHostRoleRequestV1::OracleStrategy { task, .. }
        | ProposalHostRoleRequestV1::CandidateStrategy { task, .. } => {
            task.workspace(runtime.task_limits)?
        }
    };
    Ok(FrozenProposalHostRequestV1 {
        request_id,
        request,
        runtime,
        workspace,
    })
}

#[allow(clippy::too_many_lines)]
fn drive_frozen_proposal_host_request<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    protocol_codec: NativeProtocolCodec,
    frozen: FrozenProposalHostRequestV1,
) -> Result<DrivenProposalHostRequestV1, ProposalHostError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let FrozenProposalHostRequestV1 {
        request_id,
        request,
        runtime,
        workspace,
    } = frozen;
    let terminal_request = request.clone();
    let completed = match request.role {
        ProposalHostRoleRequestV1::Sir {
            task_id,
            recovery_request,
            ..
        } => {
            let outcome = run_sir_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                SirProfileInput {
                    task_id,
                    recovery_request,
                    episode_id: runtime.episode_id,
                    model_configuration: runtime.model_configuration,
                    selection: runtime.selection,
                    budget: runtime.budget,
                    max_output_tokens: runtime.max_output_tokens,
                    task_limits: runtime.task_limits,
                },
            )
            .map_err(role_error)?;
            let crate::proposal_loop::ProposalProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            (
                ProposalHostPublicationV1::Sir {
                    proposal_id: outcome.proposal_id(),
                    proposal: outcome.proposal().clone(),
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalHostRoleRequestV1::OracleStrategy {
            workspace: oracle_workspace,
            claim,
            work_item,
            run,
            materials,
            ..
        } => {
            let outcome = run_oracle_strategy_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                OracleStrategyProfileInput {
                    workspace: oracle_workspace,
                    claim,
                    item: work_item,
                    run,
                    materials,
                    episode_id: runtime.episode_id,
                    selection: runtime.selection,
                    budget: runtime.budget,
                    max_output_tokens: runtime.max_output_tokens,
                    task_limits: runtime.task_limits,
                },
            )
            .map_err(role_error)?;
            let crate::proposal_loop::ProposalProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            let submission = outcome.submission().clone();
            (
                ProposalHostPublicationV1::OracleStrategy {
                    submission_id: submission.identity().map_err(role_error)?,
                    submission,
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalHostRoleRequestV1::CandidateStrategy {
            workspace: candidate_workspace,
            contract,
            oracle_materials,
            public_materials,
            ..
        } => {
            let outcome = run_candidate_strategy_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                &CandidateStrategyProfileInput {
                    workspace: candidate_workspace,
                    contract,
                    oracle_materials,
                    public_materials,
                    episode_id: runtime.episode_id,
                    model_configuration: runtime.model_configuration,
                    selection: runtime.selection,
                    budget: runtime.budget,
                    max_output_tokens: runtime.max_output_tokens,
                    task_limits: runtime.task_limits,
                },
            )
            .map_err(role_error)?;
            let crate::proposal_loop::ProposalProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            let proposal = outcome.proposal().clone();
            (
                ProposalHostPublicationV1::CandidateStrategy {
                    proposal_id: proposal.identity().map_err(role_error)?,
                    proposal,
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
    };
    let (publication, completion_reason, steps_started) = completed;
    Ok(DrivenProposalHostRequestV1::Complete(Box::new(
        CompletedProposalHostRequestV1 {
            request_id,
            request: terminal_request,
            episode_id: runtime.episode_id,
            publication,
            completion_reason,
            steps_started,
        },
    )))
}

fn awaiting_controller(
    request_id: ContentId<ProposalHostRequestArtifact>,
    request: ProposalHostRequestV1,
    outcome: crate::proposal_loop::ProposalProfileOutcomeV1<impl Sized>,
) -> Result<DrivenProposalHostRequestV1, ProposalHostError> {
    let crate::proposal_loop::ProposalProfileOutcomeV1::AwaitingController(experiment) = outcome
    else {
        return invalid("completed Proposal Host profile lost its publication");
    };
    Ok(DrivenProposalHostRequestV1::AwaitingController(Box::new(
        AwaitingControllerProposalHostRequestV1 {
            request_id,
            request,
            experiment,
        },
    )))
}

fn freeze_proposal_host_outcome(
    driven: DrivenProposalHostRequestV1,
) -> Result<ProposalHostOutcomeV1, ProposalHostError> {
    let DrivenProposalHostRequestV1::Complete(completed) = driven else {
        let DrivenProposalHostRequestV1::AwaitingController(awaiting) = driven else {
            unreachable!()
        };
        let AwaitingControllerProposalHostRequestV1 {
            request_id,
            request,
            experiment,
        } = *awaiting;
        let operations = experiment
            .operations
            .into_iter()
            .map(|operation| ProposalHostExperimentOperationV1 {
                operation_id: operation.operation_id,
                tool: operation.tool,
                implementation_version: operation.implementation_version,
                effect: operation.effect,
                arguments_id: operation.arguments_id,
                arguments: operation.arguments,
            })
            .collect();
        let experiment = ProposalHostExperimentRequestV1 {
            schema_version: schema_v1(),
            request: request_id,
            episode_id: experiment.episode_id,
            step_id: experiment.step_id,
            model_attempt_id: experiment.model_attempt_id,
            operations,
        };
        experiment.validate_against(&request)?;
        return Ok(ProposalHostOutcomeV1::AwaitingController { experiment });
    };
    let terminal = ProposalHostTerminalV1 {
        schema_version: schema_v1(),
        request: completed.request_id,
        episode_id: completed.episode_id,
        publication: completed.publication,
        completion_reason: completed.completion_reason,
        steps_started: completed.steps_started,
    };
    terminal.validate_against(&completed.request)?;
    Ok(ProposalHostOutcomeV1::Terminal {
        terminal: Box::new(terminal),
    })
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid schema version")
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ProposalHostError> {
    Err(ProposalHostError::InvalidRequest(message.into()))
}

fn codec(error: impl std::fmt::Display) -> ProposalHostError {
    ProposalHostError::Codec(error.to_string())
}

fn role_error(error: impl std::fmt::Display) -> ProposalHostError {
    ProposalHostError::Role(error.to_string())
}

/// Failure at the generic Host boundary without erasing role-specific diagnostics.
#[derive(Debug, Error)]
pub enum ProposalHostError {
    #[error("invalid Proposal Host request: {0}")]
    InvalidRequest(String),
    #[error("Proposal Host codec failed: {0}")]
    Codec(String),
    #[error("Proposal Host role episode failed: {0}")]
    Role(String),
}

#[cfg(test)]
mod tests {
    use cairn_agent::{ToolArguments, ToolEffectClass, ToolImplementationVersion, ToolName};
    use cairn_protocol::{ContentId, OperationId};

    use super::{ProposalHostBinaryIdentity, ProposalHostExperimentOperationV1};

    #[test]
    fn host_binary_identity_is_exact_lowercase_sha256_and_revalidated_on_decode() {
        let valid = format!("sha256:{}", "a".repeat(64));
        let identity = ProposalHostBinaryIdentity::new(valid.clone()).expect("identity");
        assert_eq!(identity.as_str(), valid);
        assert!(ProposalHostBinaryIdentity::new(format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(ProposalHostBinaryIdentity::new("sha256:short").is_err());
        assert!(
            serde_json::from_str::<ProposalHostBinaryIdentity>("\"sha256:not-a-digest\"").is_err()
        );
    }

    #[test]
    fn experiment_operation_revalidates_external_effect_and_exact_arguments() {
        let arguments = serde_json::json!({"probe":"bounded"});
        let arguments_id = ContentId::<ToolArguments>::derive(
            &cairn_codec::to_vec(&arguments).expect("arguments bytes"),
        )
        .expect("arguments identity");
        let operation = ProposalHostExperimentOperationV1 {
            operation_id: OperationId::new(),
            tool: ToolName::new("request_bounded_probe").expect("tool"),
            implementation_version: ToolImplementationVersion::new("bounded-probe-v1")
                .expect("version"),
            effect: ToolEffectClass::Idempotent,
            arguments_id,
            arguments,
        };
        let bytes = cairn_codec::to_vec(&operation).expect("operation bytes");
        let _: ProposalHostExperimentOperationV1 =
            cairn_codec::from_slice(&bytes).expect("strict operation");

        let mut value = serde_json::to_value(&operation).expect("operation value");
        value["effect"] = serde_json::json!("read-only");
        assert!(
            cairn_codec::from_slice::<ProposalHostExperimentOperationV1>(
                &cairn_codec::to_vec(&value).expect("changed bytes")
            )
            .is_err()
        );
        let mut value = serde_json::to_value(&operation).expect("operation value");
        value["arguments"]["probe"] = serde_json::json!("changed");
        assert!(
            cairn_codec::from_slice::<ProposalHostExperimentOperationV1>(
                &cairn_codec::to_vec(&value).expect("changed bytes")
            )
            .is_err()
        );
    }
}
