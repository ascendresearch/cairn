//! Generic Proposal Host boundary over domain-specific SIR and Candidate role profiles.

use cairn_agent::{
    EpisodeBudget, EpisodeCompletionReason, ModelOutputTokenLimit, ModelSelection, ModelTransport,
    NativeProtocolCodec, ToolArguments, ToolEffectClass, ToolImplementationVersion, ToolName,
};
use cairn_protocol::{
    CommandId, ContentId, ContentType, EpisodeId, ModelAttemptId, ObservedAtUnixMillis,
    OperationId, SchemaVersion, StepId, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, CandidateEpisodeKindV1, CandidateEpisodeRequestV1,
    CandidateInitialProfileInput, CandidateNativeDiagnosticV1, CandidateNativeFollowupProfileInput,
    CandidateNativePublicationV1, CandidateNativeRepairProfileInput, CandidateRevisionProfileInput,
    CandidateWorkflowStateV1, CollectionCandidateBuildDiagnosticArtifact,
    CollectionCandidateBuildDiagnosticV1, CollectionCandidateNativeBuildDiagnosticArtifact,
    CollectionCandidateNativeBuildDiagnosticV1, CollectionCandidateNativeFollowupRevisionArtifact,
    CollectionCandidateNativeFollowupRevisionV1,
    CollectionCandidateNativeRepairBuildDiagnosticArtifact,
    CollectionCandidateNativeRepairBuildDiagnosticV1,
    CollectionCandidateNativeRepairRevisionArtifact, CollectionCandidateNativeRepairRevisionV1,
    CollectionCandidateProposalArtifact, CollectionCandidateProposalV1,
    CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
    CollectionCandidateSearchInputV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, MigrationWorkflowV1, ProposalHostInvocationArtifact,
    SirCapabilityManifestV1, SirIntentHypothesisSetProposalArtifact, SirProfileInput,
    SirTaskArtifactPath, SirTaskBundleV1, SirTaskLimits, SirTaskWorkspace,
    record_candidate_native_followup, record_candidate_native_repair,
    run_candidate_initial_profile, run_candidate_native_followup_profile,
    run_candidate_native_repair_profile, run_candidate_revision_profile, run_sir_profile,
    validate_archived_candidate_build_diagnostic,
    validate_archived_candidate_native_build_diagnostic,
    validate_archived_candidate_native_repair_build_diagnostic,
    validate_archived_collection_candidate_proposal,
    validate_archived_collection_candidate_search_input,
};

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
pub enum ProposalHostRoleRequestV1 {
    Sir {
        task_id: TaskId,
        recovery_request: IntentRecoveryRequestV1,
        task: ProposalHostTaskSnapshotV1,
    },
    CandidateInitial {
        recovery_input: IntentRecoveryInputV1,
        search_input: CollectionCandidateSearchInputV1,
        task: ProposalHostTaskSnapshotV1,
    },
    CandidateRevision {
        recovery_input: IntentRecoveryInputV1,
        search_input: CollectionCandidateSearchInputV1,
        task: ProposalHostTaskSnapshotV1,
        parent: CollectionCandidateProposalV1,
        diagnostic: CollectionCandidateBuildDiagnosticV1,
    },
    CandidateNativeFollowup {
        workflow_request: CandidateEpisodeRequestV1,
        recovery_input: IntentRecoveryInputV1,
        search_input: CollectionCandidateSearchInputV1,
        task: ProposalHostTaskSnapshotV1,
        previous_revision: CollectionCandidateRevisionV1,
        diagnostic: CollectionCandidateNativeBuildDiagnosticV1,
    },
    CandidateNativeRepair {
        workflow_request: CandidateEpisodeRequestV1,
        recovery_input: IntentRecoveryInputV1,
        search_input: CollectionCandidateSearchInputV1,
        task: ProposalHostTaskSnapshotV1,
        root_followup: CollectionCandidateNativeFollowupRevisionV1,
        parent_repair: Option<Box<CollectionCandidateNativeRepairRevisionV1>>,
        diagnostic: CollectionCandidateNativeRepairBuildDiagnosticV1,
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
            | ProposalHostRoleRequestV1::CandidateInitial { task, .. }
            | ProposalHostRoleRequestV1::CandidateRevision { task, .. }
            | ProposalHostRoleRequestV1::CandidateNativeFollowup { task, .. }
            | ProposalHostRoleRequestV1::CandidateNativeRepair { task, .. } => {
                task.workspace(self.runtime.task_limits)?
            }
        };
        match &self.role {
            ProposalHostRoleRequestV1::Sir { .. } => Ok(()),
            ProposalHostRoleRequestV1::CandidateInitial {
                recovery_input,
                search_input,
                ..
            } => validate_candidate_common(&workspace, recovery_input, search_input),
            ProposalHostRoleRequestV1::CandidateRevision {
                recovery_input,
                search_input,
                parent,
                diagnostic,
                ..
            } => {
                validate_candidate_common(&workspace, recovery_input, search_input)?;
                let parent_id = parent.identity().map_err(role_error)?;
                let diagnostic_id = diagnostic_id(diagnostic)?;
                if diagnostic.parent_proposal() != parent_id {
                    return invalid("Candidate revision diagnostic changed its parent proposal");
                }
                validate_archived_collection_candidate_proposal(
                    &cairn_codec::to_vec(parent).map_err(codec)?,
                    parent_id,
                )
                .map_err(role_error)?;
                validate_archived_candidate_build_diagnostic(
                    &cairn_codec::to_vec(diagnostic).map_err(codec)?,
                    diagnostic_id,
                )
                .map_err(role_error)?;
                Ok(())
            }
            ProposalHostRoleRequestV1::CandidateNativeFollowup {
                workflow_request,
                recovery_input,
                search_input,
                previous_revision,
                diagnostic,
                ..
            } => {
                validate_candidate_common(&workspace, recovery_input, search_input)?;
                let previous_id = previous_revision.identity().map_err(role_error)?;
                let diagnostic_id = native_diagnostic_id(diagnostic)?;
                if workflow_request.episode_id() != self.runtime.episode_id
                    || workflow_request.invocation() != self.runtime.identity()?
                    || workflow_request.kind() != CandidateEpisodeKindV1::NativeFollowup
                    || workflow_request.authority().candidate_search_input()
                        != search_input.identity().map_err(role_error)?
                    || workflow_request.parent()
                        != CandidateNativePublicationV1::Revision(previous_id)
                    || workflow_request.diagnostic()
                        != CandidateNativeDiagnosticV1::NativeFollowup(diagnostic_id)
                    || diagnostic.previous_revision() != previous_id
                {
                    return invalid("Candidate native follow-up request binding changed");
                }
                Ok(())
            }
            ProposalHostRoleRequestV1::CandidateNativeRepair {
                workflow_request,
                recovery_input,
                search_input,
                root_followup,
                parent_repair,
                diagnostic,
                ..
            } => {
                validate_candidate_common(&workspace, recovery_input, search_input)?;
                let root_id = root_followup.identity().map_err(role_error)?;
                let diagnostic_id = repair_diagnostic_id(diagnostic)?;
                let expected_parent = match (workflow_request.parent(), parent_repair) {
                    (CandidateNativePublicationV1::NativeFollowup(id), None) if id == root_id => {
                        crate::CandidateNativeRepairParentV1::RootFollowup(id)
                    }
                    (CandidateNativePublicationV1::NativeRepair(id), Some(parent))
                        if parent.identity().map_err(role_error)? == id
                            && parent.root_followup() == root_id =>
                    {
                        crate::CandidateNativeRepairParentV1::Repair(id)
                    }
                    _ => return invalid("Candidate native repair parent lineage changed"),
                };
                if workflow_request.episode_id() != self.runtime.episode_id
                    || workflow_request.invocation() != self.runtime.identity()?
                    || workflow_request.kind() != CandidateEpisodeKindV1::NativeRepair
                    || workflow_request.authority().candidate_search_input()
                        != search_input.identity().map_err(role_error)?
                    || workflow_request.diagnostic()
                        != CandidateNativeDiagnosticV1::NativeRepair(diagnostic_id)
                    || root_followup.identity().map_err(role_error)? != root_id
                    || diagnostic.parent() != expected_parent
                {
                    return invalid("Candidate native repair request binding changed");
                }
                Ok(())
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
    CandidateInitial {
        proposal_id: ContentId<CollectionCandidateProposalArtifact>,
        proposal: CollectionCandidateProposalV1,
    },
    CandidateRevision {
        revision_id: ContentId<CollectionCandidateRevisionArtifact>,
        revision: CollectionCandidateRevisionV1,
    },
    CandidateNativeFollowup {
        followup_id: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
        followup: CollectionCandidateNativeFollowupRevisionV1,
    },
    CandidateNativeRepair {
        repair_id: ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
        repair: CollectionCandidateNativeRepairRevisionV1,
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
                ProposalHostRoleRequestV1::CandidateInitial { .. },
                ProposalHostPublicationV1::CandidateInitial { .. }
            ) | (
                ProposalHostRoleRequestV1::CandidateRevision { .. },
                ProposalHostPublicationV1::CandidateRevision { .. }
            ) | (
                ProposalHostRoleRequestV1::CandidateNativeFollowup { .. },
                ProposalHostPublicationV1::CandidateNativeFollowup { .. }
            ) | (
                ProposalHostRoleRequestV1::CandidateNativeRepair { .. },
                ProposalHostPublicationV1::CandidateNativeRepair { .. }
            )
        );
        if !matching_role {
            return invalid("Proposal Host terminal changed its requested role");
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
            ProposalHostPublicationV1::CandidateInitial {
                proposal_id,
                proposal,
            } => {
                if proposal.identity().map_err(role_error)? != *proposal_id {
                    return invalid("Candidate proposal identity changed");
                }
                proposal.episode_id()
            }
            ProposalHostPublicationV1::CandidateRevision {
                revision_id,
                revision,
            } => {
                if revision.identity().map_err(role_error)? != *revision_id {
                    return invalid("Candidate revision identity changed");
                }
                revision.episode_id()
            }
            ProposalHostPublicationV1::CandidateNativeFollowup {
                followup_id,
                followup,
            } => {
                if followup.identity().map_err(role_error)? != *followup_id {
                    return invalid("Candidate native follow-up identity changed");
                }
                followup.episode_id()
            }
            ProposalHostPublicationV1::CandidateNativeRepair { repair_id, repair } => {
                if repair.identity().map_err(role_error)? != *repair_id {
                    return invalid("Candidate native repair identity changed");
                }
                repair.episode_id()
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
        | ProposalHostRoleRequestV1::CandidateInitial { task, .. }
        | ProposalHostRoleRequestV1::CandidateRevision { task, .. }
        | ProposalHostRoleRequestV1::CandidateNativeFollowup { task, .. }
        | ProposalHostRoleRequestV1::CandidateNativeRepair { task, .. } => {
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
        ProposalHostRoleRequestV1::CandidateInitial {
            recovery_input,
            search_input,
            ..
        } => {
            let search = prepared_search(&search_input)?;
            let outcome = run_candidate_initial_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateInitialProfileInput {
                    search_input: search,
                    recovery_input,
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
                ProposalHostPublicationV1::CandidateInitial {
                    proposal_id: outcome.proposal_id(),
                    proposal: outcome.proposal().clone(),
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalHostRoleRequestV1::CandidateRevision {
            recovery_input,
            search_input,
            parent,
            diagnostic,
            ..
        } => {
            let parent_id = parent.identity().map_err(role_error)?;
            let diagnostic_id = diagnostic_id(&diagnostic)?;
            let outcome = run_candidate_revision_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateRevisionProfileInput {
                    search_input: prepared_search(&search_input)?,
                    recovery_input,
                    parent,
                    parent_id,
                    build_diagnostic: validate_archived_candidate_build_diagnostic(
                        &cairn_codec::to_vec(&diagnostic).map_err(codec)?,
                        diagnostic_id,
                    )
                    .map_err(role_error)?,
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
                ProposalHostPublicationV1::CandidateRevision {
                    revision_id: outcome.revision_id(),
                    revision: outcome.revision().clone(),
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalHostRoleRequestV1::CandidateNativeFollowup {
            recovery_input,
            search_input,
            previous_revision,
            diagnostic,
            ..
        } => {
            let previous_revision_id = previous_revision.identity().map_err(role_error)?;
            let diagnostic_id = native_diagnostic_id(&diagnostic)?;
            let outcome = run_candidate_native_followup_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateNativeFollowupProfileInput {
                    search_input: prepared_search(&search_input)?,
                    recovery_input,
                    previous_revision,
                    previous_revision_id,
                    build_diagnostic: validate_archived_candidate_native_build_diagnostic(
                        &cairn_codec::to_vec(&diagnostic).map_err(codec)?,
                        diagnostic_id,
                    )
                    .map_err(role_error)?,
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
                ProposalHostPublicationV1::CandidateNativeFollowup {
                    followup_id: outcome.followup_id(),
                    followup: outcome.followup().clone(),
                },
                outcome.completion_reason(),
                outcome.steps_started(),
            )
        }
        ProposalHostRoleRequestV1::CandidateNativeRepair {
            recovery_input,
            search_input,
            root_followup,
            parent_repair: _,
            diagnostic,
            ..
        } => {
            let root_followup_id = root_followup.identity().map_err(role_error)?;
            let diagnostic_id = repair_diagnostic_id(&diagnostic)?;
            let outcome = run_candidate_native_repair_profile(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateNativeRepairProfileInput {
                    search_input: prepared_search(&search_input)?,
                    recovery_input,
                    root_followup,
                    root_followup_id,
                    build_diagnostic: validate_archived_candidate_native_repair_build_diagnostic(
                        &cairn_codec::to_vec(&diagnostic).map_err(codec)?,
                        diagnostic_id,
                    )
                    .map_err(role_error)?,
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
                ProposalHostPublicationV1::CandidateNativeRepair {
                    repair_id: outcome.repair_id(),
                    repair: outcome.repair().clone(),
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

/// Records one validated Candidate Host publication into the exact task-owned workflow.
///
/// # Errors
///
/// Rejects non-Candidate roles, request/terminal drift, wrong publication kinds, or any workflow
/// transition and persistence failure.
pub fn record_candidate_proposal_host_terminal<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    request: &ProposalHostRequestV1,
    terminal: &ProposalHostTerminalV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, ProposalHostError> {
    terminal.validate_against(request)?;
    match (&request.role, &terminal.publication) {
        (
            ProposalHostRoleRequestV1::CandidateNativeFollowup {
                workflow_request, ..
            },
            ProposalHostPublicationV1::CandidateNativeFollowup {
                followup_id,
                followup,
            },
        ) => {
            if workflow_request.episode_id() != terminal.episode_id {
                return invalid("Candidate follow-up terminal changed its workflow episode");
            }
            record_candidate_native_followup(
                events,
                workflow,
                followup,
                *followup_id,
                command_id,
                observed_at,
            )
            .map_err(workflow_error)
        }
        (
            ProposalHostRoleRequestV1::CandidateNativeRepair {
                workflow_request, ..
            },
            ProposalHostPublicationV1::CandidateNativeRepair { repair_id, repair },
        ) => {
            if workflow_request.episode_id() != terminal.episode_id {
                return invalid("Candidate repair terminal changed its workflow episode");
            }
            record_candidate_native_repair(
                events,
                workflow,
                repair,
                *repair_id,
                command_id,
                observed_at,
            )
            .map_err(workflow_error)
        }
        _ => invalid("Proposal Host terminal is not a workflow Candidate publication"),
    }
}

fn validate_candidate_common(
    workspace: &SirTaskWorkspace,
    recovery: &IntentRecoveryInputV1,
    search: &CollectionCandidateSearchInputV1,
) -> Result<(), ProposalHostError> {
    if workspace.bundle().identity().map_err(role_error)? != recovery.task_bundle()
        || recovery.identity().map_err(role_error)? != search.recovery_input()
        || recovery.task_id() != search.task_id()
    {
        return invalid("Candidate task, recovery, and search input binding changed");
    }
    prepared_search(search)?;
    Ok(())
}

fn prepared_search(
    search: &CollectionCandidateSearchInputV1,
) -> Result<crate::PreparedCollectionCandidateSearchInput, ProposalHostError> {
    let bytes = cairn_codec::to_vec(search).map_err(codec)?;
    let id = search.identity().map_err(role_error)?;
    validate_archived_collection_candidate_search_input(&bytes, id).map_err(role_error)
}

fn diagnostic_id(
    diagnostic: &CollectionCandidateBuildDiagnosticV1,
) -> Result<ContentId<CollectionCandidateBuildDiagnosticArtifact>, ProposalHostError> {
    ContentId::derive(&cairn_codec::to_vec(diagnostic).map_err(codec)?).map_err(codec)
}

fn native_diagnostic_id(
    diagnostic: &CollectionCandidateNativeBuildDiagnosticV1,
) -> Result<ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>, ProposalHostError> {
    ContentId::derive(&cairn_codec::to_vec(diagnostic).map_err(codec)?).map_err(codec)
}

fn repair_diagnostic_id(
    diagnostic: &CollectionCandidateNativeRepairBuildDiagnosticV1,
) -> Result<ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>, ProposalHostError> {
    ContentId::derive(&cairn_codec::to_vec(diagnostic).map_err(codec)?).map_err(codec)
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

fn workflow_error(error: impl std::fmt::Display) -> ProposalHostError {
    ProposalHostError::Workflow(error.to_string())
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
    #[error("Proposal Host workflow publication failed: {0}")]
    Workflow(String),
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
