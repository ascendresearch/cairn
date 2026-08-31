//! Generic Proposal step boundary over domain-specific SIR and Candidate role profiles.

use cairn_agent::{
    EpisodeBudget, EpisodeCompletionReason, ModelOutputTokenLimit, ModelSelection, ModelTransport,
    NativeProtocolCodec, ToolArguments, ToolEffectClass, ToolImplementationVersion, ToolName,
};
use cairn_protocol::{
    ContentId, ContentType, EpisodeId, ModelAttemptId, OperationId, SchemaVersion, StepId, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, AgentRuntimeBindingArtifact, CandidateOracleContractV1,
    CandidateOracleMaterialsV1, CandidateProposalArtifact, CandidateProposalV1,
    CandidateStrategyProfileInput, CandidateWorkspaceV1, IntentHypothesisSetProposalV1,
    IntentRecoveryInputV1, IntentRecoveryRequestV1, OracleBuildTestSnapshotArtifact, OracleClaimV1,
    OracleDocumentationSnapshotArtifact, OracleKnowledgeSnapshotArtifact, OracleStrategyExecutorV1,
    OracleStrategyProfileInput, OracleStrategyRunV1, OracleStrategySubmissionArtifact,
    OracleStrategySubmissionV1, OracleStrategyToolCatalogV1, OracleWorkItemV1, OracleWorkspaceV1,
    SirCapabilityManifestV1, SirIntentHypothesisSetProposalArtifact, SirProfileInput,
    SirTaskArtifactPath, SirTaskBundleV1, SirTaskLimits, SirTaskWorkspace,
    run_candidate_strategy_profile, run_oracle_strategy_profile, run_sir_profile,
};

/// Canonical request accepted by one generic Proposal step episode.
pub enum ProposalStepRequestArtifact {}

impl ContentType for ProposalStepRequestArtifact {
    const DOMAIN: &'static str = "migration.proposal-step-request.v1";
}

/// Canonical terminal outcome returned by one generic Proposal step episode.
pub enum ProposalStepTerminalArtifact {}

impl ContentType for ProposalStepTerminalArtifact {
    const DOMAIN: &'static str = "migration.proposal-step-terminal.v1";
}

/// Canonical request identity returned before Controller-owned Worker execution.
///
/// ```compile_fail
/// use cairn_migration::{WorkflowToolRequestArtifact, ProposalStepTerminalArtifact};
/// use cairn_protocol::ContentId;
/// fn require_experiment(_: ContentId<WorkflowToolRequestArtifact>) {}
/// fn invalid(terminal: ContentId<ProposalStepTerminalArtifact>) {
///     require_experiment(terminal);
/// }
/// ```
pub enum WorkflowToolRequestArtifact {}

impl ContentType for WorkflowToolRequestArtifact {
    const DOMAIN: &'static str = "migration.workflow-tool-request.v1";
}

/// One exact source entry in a Controller-materialized task snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalStepTaskSourceV1 {
    path: SirTaskArtifactPath,
    source: String,
}

impl ProposalStepTaskSourceV1 {
    #[must_use]
    pub fn new(path: SirTaskArtifactPath, source: String) -> Self {
        Self { path, source }
    }

    #[must_use]
    pub const fn path(&self) -> &SirTaskArtifactPath {
        &self.path
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Exact task material projected into one proposal step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalStepTaskSnapshotV1 {
    bundle: SirTaskBundleV1,
    sources: Vec<ProposalStepTaskSourceV1>,
}

const ORACLE_STEP_TEXT_SNAPSHOT_LIMIT: usize = 512 * 1024;

macro_rules! oracle_step_text_snapshot {
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
            /// Constructs one exact UTF-8 snapshot consumed by the proposal step.
            ///
            /// # Errors
            ///
            /// Rejects empty/oversized text or identity drift from the frozen workspace edge.
            pub fn new(
                identity: ContentId<$artifact>,
                text: String,
            ) -> Result<Self, ProposalStepError> {
                if text.is_empty()
                    || text.len() > ORACLE_STEP_TEXT_SNAPSHOT_LIMIT
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
            type Error = ProposalStepError;

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

oracle_step_text_snapshot!(
    ProposalStepOracleDocumentationV1,
    ProposalStepOracleDocumentationWire,
    OracleDocumentationSnapshotArtifact,
    "Oracle documentation"
);
oracle_step_text_snapshot!(
    ProposalStepOracleBuildTestsV1,
    ProposalStepOracleBuildTestsWire,
    OracleBuildTestSnapshotArtifact,
    "Oracle build/test"
);
oracle_step_text_snapshot!(
    ProposalStepOracleKnowledgeV1,
    ProposalStepOracleKnowledgeWire,
    OracleKnowledgeSnapshotArtifact,
    "Oracle knowledge"
);

/// Exact human-readable public material projected into one Oracle strategy step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalStepOracleMaterialsV1 {
    documentation: ProposalStepOracleDocumentationV1,
    build_and_tests: ProposalStepOracleBuildTestsV1,
    knowledge: ProposalStepOracleKnowledgeV1,
}

impl ProposalStepOracleMaterialsV1 {
    #[must_use]
    pub fn new(
        documentation: ProposalStepOracleDocumentationV1,
        build_and_tests: ProposalStepOracleBuildTestsV1,
        knowledge: ProposalStepOracleKnowledgeV1,
    ) -> Self {
        Self {
            documentation,
            build_and_tests,
            knowledge,
        }
    }

    #[must_use]
    pub const fn documentation(&self) -> &ProposalStepOracleDocumentationV1 {
        &self.documentation
    }

    #[must_use]
    pub const fn build_and_tests(&self) -> &ProposalStepOracleBuildTestsV1 {
        &self.build_and_tests
    }

    #[must_use]
    pub const fn knowledge(&self) -> &ProposalStepOracleKnowledgeV1 {
        &self.knowledge
    }
}

impl ProposalStepTaskSnapshotV1 {
    #[must_use]
    pub fn new(bundle: SirTaskBundleV1, sources: Vec<ProposalStepTaskSourceV1>) -> Self {
        Self { bundle, sources }
    }

    #[must_use]
    pub const fn bundle(&self) -> &SirTaskBundleV1 {
        &self.bundle
    }

    #[must_use]
    pub fn sources(&self) -> &[ProposalStepTaskSourceV1] {
        &self.sources
    }

    /// Copies the exact already-validated workspace into a step-owned snapshot.
    #[must_use]
    pub fn from_workspace(workspace: &SirTaskWorkspace) -> Self {
        Self {
            bundle: workspace.bundle().clone(),
            sources: workspace
                .materialized_sources()
                .into_iter()
                .map(|(path, source)| ProposalStepTaskSourceV1 { path, source })
                .collect(),
        }
    }

    fn workspace(&self, limits: SirTaskLimits) -> Result<SirTaskWorkspace, ProposalStepError> {
        SirTaskWorkspace::from_materialized(
            self.bundle.clone(),
            self.sources
                .iter()
                .map(|source| (source.path.clone(), source.source.clone()))
                .collect(),
            limits,
        )
        .map_err(|error| ProposalStepError::InvalidRequest(error.to_string()))
    }
}

/// Runtime facts frozen before a Proposal step may dispatch a model effect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalStepRuntimeV1 {
    schema_version: SchemaVersion,
    episode_id: EpisodeId,
    model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
    selection: ModelSelection,
    budget: EpisodeBudget,
    max_output_tokens: ModelOutputTokenLimit,
    task_limits: SirTaskLimits,
}

impl ProposalStepRuntimeV1 {
    #[must_use]
    pub fn new(
        episode_id: EpisodeId,
        model_configuration: ContentId<AgentResolvedRuntimeModelArtifact>,
        selection: ModelSelection,
        budget: EpisodeBudget,
        max_output_tokens: ModelOutputTokenLimit,
        task_limits: SirTaskLimits,
    ) -> Self {
        Self {
            schema_version: schema_v1(),
            episode_id,
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
    pub fn identity(&self) -> Result<ContentId<AgentRuntimeBindingArtifact>, ProposalStepError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal step runtime is not current V1");
        }
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }
}

/// Closed set of domain-specific inputs consumed by the main workflow proposal step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "role")]
#[allow(clippy::large_enum_variant)]
pub enum ProposalStepRoleRequestV1 {
    Sir {
        task_id: TaskId,
        recovery_request: IntentRecoveryRequestV1,
        task: ProposalStepTaskSnapshotV1,
    },
    OracleStrategy {
        workspace: OracleWorkspaceV1,
        claim: OracleClaimV1,
        work_item: OracleWorkItemV1,
        run: OracleStrategyRunV1,
        task: ProposalStepTaskSnapshotV1,
        materials: ProposalStepOracleMaterialsV1,
    },
    CandidateStrategy {
        workspace: CandidateWorkspaceV1,
        contract: CandidateOracleContractV1,
        oracle_materials: CandidateOracleMaterialsV1,
        task: ProposalStepTaskSnapshotV1,
        public_materials: ProposalStepOracleMaterialsV1,
    },
}

/// Exact current-V1 proposal-step request. Deserialization reruns every role binding invariant.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalStepRequestV1 {
    schema_version: SchemaVersion,
    runtime: ProposalStepRuntimeV1,
    role: ProposalStepRoleRequestV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalStepRequestWire {
    schema_version: SchemaVersion,
    runtime: ProposalStepRuntimeV1,
    role: ProposalStepRoleRequestV1,
}

impl ProposalStepRequestV1 {
    /// Creates and validates one exact proposal-step request.
    ///
    /// # Errors
    ///
    /// Rejects task, recovery, parent, diagnostic, workflow, or episode binding drift.
    pub fn new(
        runtime: ProposalStepRuntimeV1,
        role: ProposalStepRoleRequestV1,
    ) -> Result<Self, ProposalStepError> {
        let value = Self {
            schema_version: schema_v1(),
            runtime,
            role,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn runtime(&self) -> &ProposalStepRuntimeV1 {
        &self.runtime
    }

    #[must_use]
    pub const fn role(&self) -> &ProposalStepRoleRequestV1 {
        &self.role
    }

    /// Reconstructs the exact SIR input frozen by this proposal-step request.
    ///
    /// # Errors
    ///
    /// Rejects a non-SIR role or an invalid task/capability binding.
    pub fn sir_recovery_input(&self) -> Result<IntentRecoveryInputV1, ProposalStepError> {
        self.validate()?;
        let ProposalStepRoleRequestV1::Sir {
            task_id,
            recovery_request,
            task,
        } = &self.role
        else {
            return invalid("Proposal step request is not an SIR role");
        };
        IntentRecoveryInputV1::new(
            *task_id,
            task.bundle.identity().map_err(role_error)?,
            recovery_request.clone(),
            SirCapabilityManifestV1::proposal_only(self.runtime.task_limits),
        )
        .map_err(role_error)
    }

    /// Derives the exact request identity used by the step terminal.
    ///
    /// # Errors
    ///
    /// Returns an error if validation or canonical encoding fails.
    pub fn identity(&self) -> Result<ContentId<ProposalStepRequestArtifact>, ProposalStepError> {
        self.validate()?;
        let bytes = cairn_codec::to_vec(self).map_err(codec)?;
        ContentId::derive(&bytes).map_err(codec)
    }

    #[allow(clippy::too_many_lines)]
    fn validate(&self) -> Result<(), ProposalStepError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal step request is not current V1");
        }
        let workspace = match &self.role {
            ProposalStepRoleRequestV1::Sir { task, .. }
            | ProposalStepRoleRequestV1::OracleStrategy { task, .. }
            | ProposalStepRoleRequestV1::CandidateStrategy { task, .. } => {
                task.workspace(self.runtime.task_limits)?
            }
        };
        match &self.role {
            ProposalStepRoleRequestV1::Sir { .. } => Ok(()),
            ProposalStepRoleRequestV1::OracleStrategy {
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
                let OracleStrategyExecutorV1::AgentStep {
                    invocation, tools, ..
                } = run.executor()
                else {
                    return invalid("Oracle Proposal step request names a non-Agent executor");
                };
                if *invocation != self.runtime.identity()?
                    || *tools
                        != OracleStrategyToolCatalogV1::standard()
                            .identity()
                            .map_err(role_error)?
                {
                    return invalid(
                        "Oracle strategy run changed its Proposal step invocation or tool catalog",
                    );
                }
                Ok(())
            }
            ProposalStepRoleRequestV1::CandidateStrategy {
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

impl TryFrom<ProposalStepRequestWire> for ProposalStepRequestV1 {
    type Error = ProposalStepError;

    fn try_from(wire: ProposalStepRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            runtime: wire.runtime,
            role: wire.role,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalStepRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalStepRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Exact external operation proposed by an Agent and durably bound by the Proposal step.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkflowToolOperationV1 {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
    arguments: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowToolOperationWire {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<ToolArguments>,
    arguments: serde_json::Value,
}

impl WorkflowToolOperationV1 {
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

    fn validate(&self) -> Result<(), ProposalStepError> {
        if matches!(
            self.effect,
            ToolEffectClass::Pure | ToolEffectClass::ReadOnly
        ) {
            return invalid("Proposal step Worker request contains a workflow-local operation");
        }
        let bytes = cairn_codec::to_vec(&self.arguments).map_err(codec)?;
        if ContentId::<ToolArguments>::derive(&bytes).map_err(codec)? != self.arguments_id {
            return invalid(
                "Proposal step Worker request arguments changed their content identity",
            );
        }
        Ok(())
    }
}

impl TryFrom<WorkflowToolOperationWire> for WorkflowToolOperationV1 {
    type Error = ProposalStepError;

    fn try_from(wire: WorkflowToolOperationWire) -> Result<Self, Self::Error> {
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

impl<'de> Deserialize<'de> for WorkflowToolOperationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WorkflowToolOperationWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Request-bound safe point at which only the Controller may authorize Worker execution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkflowToolRequestV1 {
    schema_version: SchemaVersion,
    request: ContentId<ProposalStepRequestArtifact>,
    episode_id: EpisodeId,
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    operations: Vec<WorkflowToolOperationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowToolRequestWire {
    schema_version: SchemaVersion,
    request: ContentId<ProposalStepRequestArtifact>,
    episode_id: EpisodeId,
    step_id: StepId,
    model_attempt_id: ModelAttemptId,
    operations: Vec<WorkflowToolOperationV1>,
}

impl WorkflowToolRequestV1 {
    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalStepRequestArtifact> {
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
    pub fn operations(&self) -> &[WorkflowToolOperationV1] {
        &self.operations
    }

    /// Derives the exact Worker-request identity.
    ///
    /// # Errors
    ///
    /// Rejects an invalid structure or canonical codec/content identity failure.
    pub fn identity(&self) -> Result<ContentId<WorkflowToolRequestArtifact>, ProposalStepError> {
        self.validate_structure()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }

    /// Revalidates this Worker request against the proposal step that opened the episode.
    ///
    /// # Errors
    ///
    /// Rejects structure, request identity, or episode identity drift.
    pub fn validate_against(
        &self,
        request: &ProposalStepRequestV1,
    ) -> Result<(), ProposalStepError> {
        self.validate_structure()?;
        if self.request != request.identity()? || self.episode_id != request.runtime.episode_id {
            return invalid("Proposal step Worker request changed its request or episode identity");
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), ProposalStepError> {
        if self.schema_version != schema_v1() || self.operations.is_empty() {
            return invalid("Proposal step Worker request structure is invalid");
        }
        let mut ids = std::collections::HashSet::new();
        for operation in &self.operations {
            operation.validate()?;
            if !ids.insert(operation.operation_id) {
                return invalid("Proposal step Worker request repeats an operation identity");
            }
        }
        Ok(())
    }
}

impl TryFrom<WorkflowToolRequestWire> for WorkflowToolRequestV1 {
    type Error = ProposalStepError;

    fn try_from(wire: WorkflowToolRequestWire) -> Result<Self, Self::Error> {
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

impl<'de> Deserialize<'de> for WorkflowToolRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        WorkflowToolRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Step result: either a terminal proposal or a typed Worker request for the Controller.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "outcome")]
pub enum ProposalStepOutcomeV1 {
    Terminal {
        terminal: Box<ProposalStepTerminalV1>,
    },
    WorkerRequest {
        request: WorkflowToolRequestV1,
    },
}

impl ProposalStepOutcomeV1 {
    /// Revalidates either current outcome against the exact proposal-step request.
    ///
    /// # Errors
    ///
    /// Rejects terminal or Worker request/episode/role/binding drift.
    pub fn validate_against(
        &self,
        request: &ProposalStepRequestV1,
    ) -> Result<(), ProposalStepError> {
        match self {
            Self::Terminal { terminal } => terminal.validate_against(request),
            Self::WorkerRequest {
                request: worker_request,
            } => worker_request.validate_against(request),
        }
    }
}

/// Typed proposal publication produced by a main-workflow proposal step.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "role")]
pub enum ProposalStepPublicationV1 {
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

/// Exact terminal result bound to one proposal-step request and durable agent episode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalStepTerminalV1 {
    schema_version: SchemaVersion,
    request: ContentId<ProposalStepRequestArtifact>,
    episode_id: EpisodeId,
    publication: ProposalStepPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalStepTerminalWire {
    schema_version: SchemaVersion,
    request: ContentId<ProposalStepRequestArtifact>,
    episode_id: EpisodeId,
    publication: ProposalStepPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

impl ProposalStepTerminalV1 {
    #[must_use]
    pub const fn request(&self) -> ContentId<ProposalStepRequestArtifact> {
        self.request
    }

    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    #[must_use]
    pub const fn publication(&self) -> &ProposalStepPublicationV1 {
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
    pub fn identity(&self) -> Result<ContentId<ProposalStepTerminalArtifact>, ProposalStepError> {
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
        request: &ProposalStepRequestV1,
    ) -> Result<(), ProposalStepError> {
        self.validate_structure()?;
        if self.request != request.identity()? || self.episode_id != request.runtime.episode_id {
            return invalid("Proposal step terminal changed its request or episode identity");
        }
        let matching_role = matches!(
            (&request.role, &self.publication),
            (
                ProposalStepRoleRequestV1::Sir { .. },
                ProposalStepPublicationV1::Sir { .. }
            ) | (
                ProposalStepRoleRequestV1::OracleStrategy { .. },
                ProposalStepPublicationV1::OracleStrategy { .. }
            ) | (
                ProposalStepRoleRequestV1::CandidateStrategy { .. },
                ProposalStepPublicationV1::CandidateStrategy { .. }
            )
        );
        if !matching_role {
            return invalid("Proposal step terminal changed its requested role");
        }
        if let (
            ProposalStepRoleRequestV1::OracleStrategy { run, work_item, .. },
            ProposalStepPublicationV1::OracleStrategy { submission, .. },
        ) = (&request.role, &self.publication)
        {
            if submission.run() != run.identity().map_err(role_error)?
                || submission.item() != work_item.identity().map_err(role_error)?
            {
                return invalid("Oracle strategy publication changed its run or work item");
            }
        }
        if let (
            ProposalStepRoleRequestV1::CandidateStrategy { contract, .. },
            ProposalStepPublicationV1::CandidateStrategy { proposal, .. },
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

    fn validate_structure(&self) -> Result<(), ProposalStepError> {
        if self.schema_version != schema_v1() || self.steps_started == 0 {
            return invalid("Proposal step terminal structure is invalid");
        }
        let episode_id = match &self.publication {
            ProposalStepPublicationV1::Sir {
                proposal_id,
                proposal,
            } => {
                if proposal.identity().map_err(role_error)? != *proposal_id {
                    return invalid("SIR publication identity changed");
                }
                proposal.episode_id()
            }
            ProposalStepPublicationV1::OracleStrategy {
                submission_id,
                submission,
            } => {
                if submission.identity().map_err(role_error)? != *submission_id {
                    return invalid("Oracle strategy submission identity changed");
                }
                self.episode_id
            }
            ProposalStepPublicationV1::CandidateStrategy {
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
            return invalid("Proposal step publication changed its episode identity");
        }
        Ok(())
    }
}

impl TryFrom<ProposalStepTerminalWire> for ProposalStepTerminalV1 {
    type Error = ProposalStepError;

    fn try_from(wire: ProposalStepTerminalWire) -> Result<Self, Self::Error> {
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

impl<'de> Deserialize<'de> for ProposalStepTerminalV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        ProposalStepTerminalWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

struct FrozenProposalStepRequestV1 {
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: ProposalStepRequestV1,
    runtime: ProposalStepRuntimeV1,
    workspace: SirTaskWorkspace,
}

struct CompletedProposalStepRequestV1 {
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: ProposalStepRequestV1,
    episode_id: EpisodeId,
    publication: ProposalStepPublicationV1,
    completion_reason: EpisodeCompletionReason,
    steps_started: u32,
}

enum DrivenProposalStepRequestV1 {
    Complete(Box<CompletedProposalStepRequestV1>),
    WorkerRequest(Box<WorkerRequestProposalStepRequestV1>),
}

struct WorkerRequestProposalStepRequestV1 {
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: ProposalStepRequestV1,
    worker_request: cairn_agent::AgentWorkerRequestV1,
}

/// Processes any supported request through the single frozen-input Proposal step lifecycle.
///
/// # Errors
///
/// Rejects invalid frozen material, durable Agent Loop failures, invalid typed submissions, or a
/// terminal outcome that does not bind to the exact request.
pub fn run_proposal_step_episode<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    protocol_codec: NativeProtocolCodec,
    request: ProposalStepRequestV1,
) -> Result<ProposalStepOutcomeV1, ProposalStepError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let frozen = freeze_proposal_step_request(request)?;
    let driven =
        drive_frozen_proposal_step_request(events, content, transport, protocol_codec, frozen)?;
    freeze_proposal_step_outcome(driven)
}

fn freeze_proposal_step_request(
    request: ProposalStepRequestV1,
) -> Result<FrozenProposalStepRequestV1, ProposalStepError> {
    request.validate()?;
    let request_id = request.identity()?;
    let runtime = request.runtime.clone();
    let workspace = match &request.role {
        ProposalStepRoleRequestV1::Sir { task, .. }
        | ProposalStepRoleRequestV1::OracleStrategy { task, .. }
        | ProposalStepRoleRequestV1::CandidateStrategy { task, .. } => {
            task.workspace(runtime.task_limits)?
        }
    };
    Ok(FrozenProposalStepRequestV1 {
        request_id,
        request,
        runtime,
        workspace,
    })
}

#[allow(clippy::too_many_lines)]
fn drive_frozen_proposal_step_request<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    protocol_codec: NativeProtocolCodec,
    frozen: FrozenProposalStepRequestV1,
) -> Result<DrivenProposalStepRequestV1, ProposalStepError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    let FrozenProposalStepRequestV1 {
        request_id,
        request,
        runtime,
        workspace,
    } = frozen;
    let terminal_request = request.clone();
    let completed = match request.role {
        ProposalStepRoleRequestV1::Sir {
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
            let cairn_agent::AgentProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            (
                ProposalStepPublicationV1::Sir {
                    proposal_id: outcome.proposal_id(),
                    proposal: outcome.proposal().clone(),
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalStepRoleRequestV1::OracleStrategy {
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
            let cairn_agent::AgentProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            let submission = outcome.submission().clone();
            (
                ProposalStepPublicationV1::OracleStrategy {
                    submission_id: submission.identity().map_err(role_error)?,
                    submission,
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalStepRoleRequestV1::CandidateStrategy {
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
            let cairn_agent::AgentProfileOutcomeV1::Complete(outcome) = outcome else {
                return awaiting_controller(request_id, terminal_request, outcome);
            };
            let proposal = outcome.proposal().clone();
            (
                ProposalStepPublicationV1::CandidateStrategy {
                    proposal_id: proposal.identity().map_err(role_error)?,
                    proposal,
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
    };
    let (publication, completion_reason, steps_started) = completed;
    Ok(DrivenProposalStepRequestV1::Complete(Box::new(
        CompletedProposalStepRequestV1 {
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
    request_id: ContentId<ProposalStepRequestArtifact>,
    request: ProposalStepRequestV1,
    outcome: cairn_agent::AgentProfileOutcomeV1<impl Sized>,
) -> Result<DrivenProposalStepRequestV1, ProposalStepError> {
    let cairn_agent::AgentProfileOutcomeV1::WorkerRequest(worker_request) = outcome else {
        return invalid("completed Proposal step profile lost its publication");
    };
    Ok(DrivenProposalStepRequestV1::WorkerRequest(Box::new(
        WorkerRequestProposalStepRequestV1 {
            request_id,
            request,
            worker_request,
        },
    )))
}

fn freeze_proposal_step_outcome(
    driven: DrivenProposalStepRequestV1,
) -> Result<ProposalStepOutcomeV1, ProposalStepError> {
    let DrivenProposalStepRequestV1::Complete(completed) = driven else {
        let DrivenProposalStepRequestV1::WorkerRequest(awaiting) = driven else {
            unreachable!()
        };
        let WorkerRequestProposalStepRequestV1 {
            request_id,
            request,
            worker_request,
        } = *awaiting;
        let operations = worker_request
            .operations
            .into_iter()
            .map(|operation| WorkflowToolOperationV1 {
                operation_id: operation.operation_id,
                tool: operation.tool,
                implementation_version: operation.implementation_version,
                effect: operation.effect,
                arguments_id: operation.arguments_id,
                arguments: operation.arguments,
            })
            .collect();
        let worker_request = WorkflowToolRequestV1 {
            schema_version: schema_v1(),
            request: request_id,
            episode_id: worker_request.episode_id,
            step_id: worker_request.step_id,
            model_attempt_id: worker_request.model_attempt_id,
            operations,
        };
        worker_request.validate_against(&request)?;
        return Ok(ProposalStepOutcomeV1::WorkerRequest {
            request: worker_request,
        });
    };
    let terminal = ProposalStepTerminalV1 {
        schema_version: schema_v1(),
        request: completed.request_id,
        episode_id: completed.episode_id,
        publication: completed.publication,
        completion_reason: completed.completion_reason,
        steps_started: completed.steps_started,
    };
    terminal.validate_against(&completed.request)?;
    Ok(ProposalStepOutcomeV1::Terminal {
        terminal: Box::new(terminal),
    })
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("current V1 is a valid schema version")
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ProposalStepError> {
    Err(ProposalStepError::InvalidRequest(message.into()))
}

fn codec(error: impl std::fmt::Display) -> ProposalStepError {
    ProposalStepError::Codec(error.to_string())
}

fn role_error(error: impl std::fmt::Display) -> ProposalStepError {
    ProposalStepError::Role(error.to_string())
}

/// Failure while executing a proposal step without erasing role-specific diagnostics.
#[derive(Debug, Error)]
pub enum ProposalStepError {
    #[error("invalid Proposal step request: {0}")]
    InvalidRequest(String),
    #[error("Proposal step codec failed: {0}")]
    Codec(String),
    #[error("Proposal step role episode failed: {0}")]
    Role(String),
}

#[cfg(test)]
mod tests {
    use cairn_agent::{ToolArguments, ToolEffectClass, ToolImplementationVersion, ToolName};
    use cairn_protocol::{ContentId, OperationId};

    use super::WorkflowToolOperationV1;

    #[test]
    fn worker_operation_request_revalidates_external_effect_and_exact_arguments() {
        let arguments = serde_json::json!({"probe":"bounded"});
        let arguments_id = ContentId::<ToolArguments>::derive(
            &cairn_codec::to_vec(&arguments).expect("arguments bytes"),
        )
        .expect("arguments identity");
        let operation = WorkflowToolOperationV1 {
            operation_id: OperationId::new(),
            tool: ToolName::new("request_bounded_probe").expect("tool"),
            implementation_version: ToolImplementationVersion::new("bounded-probe-v1")
                .expect("version"),
            effect: ToolEffectClass::Idempotent,
            arguments_id,
            arguments,
        };
        let bytes = cairn_codec::to_vec(&operation).expect("operation bytes");
        let _: WorkflowToolOperationV1 = cairn_codec::from_slice(&bytes).expect("strict operation");

        let mut value = serde_json::to_value(&operation).expect("operation value");
        value["effect"] = serde_json::json!("read-only");
        assert!(
            cairn_codec::from_slice::<WorkflowToolOperationV1>(
                &cairn_codec::to_vec(&value).expect("changed bytes")
            )
            .is_err()
        );
        let mut value = serde_json::to_value(&operation).expect("operation value");
        value["arguments"]["probe"] = serde_json::json!("changed");
        assert!(
            cairn_codec::from_slice::<WorkflowToolOperationV1>(
                &cairn_codec::to_vec(&value).expect("changed bytes")
            )
            .is_err()
        );
    }
}
