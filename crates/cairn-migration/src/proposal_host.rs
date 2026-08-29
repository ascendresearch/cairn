//! Generic Proposal Host boundary over domain-specific SIR and Candidate role profiles.

use cairn_agent::{
    EpisodeBudget, EpisodeCompletionReason, ModelOutputTokenLimit, ModelSelection, ModelTransport,
    NativeProtocolCodec,
};
use cairn_protocol::{
    CommandId, ContentId, ContentType, EpisodeId, ObservedAtUnixMillis, SchemaVersion, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::{
    AgentResolvedRuntimeModelArtifact, CandidateEpisodeKindV1, CandidateEpisodeRequestV1,
    CandidateEpisodeRunInput, CandidateNativeDiagnosticV1, CandidateNativeFollowupEpisodeRunInput,
    CandidateNativePublicationV1, CandidateNativeRepairEpisodeRunInput,
    CandidateRevisionEpisodeRunInput, CandidateWorkflowStateV1,
    CollectionCandidateBuildDiagnosticArtifact, CollectionCandidateBuildDiagnosticV1,
    CollectionCandidateNativeBuildDiagnosticArtifact, CollectionCandidateNativeBuildDiagnosticV1,
    CollectionCandidateNativeFollowupRevisionArtifact, CollectionCandidateNativeFollowupRevisionV1,
    CollectionCandidateNativeRepairBuildDiagnosticArtifact,
    CollectionCandidateNativeRepairBuildDiagnosticV1,
    CollectionCandidateNativeRepairRevisionArtifact, CollectionCandidateNativeRepairRevisionV1,
    CollectionCandidateProposalArtifact, CollectionCandidateProposalV1,
    CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
    CollectionCandidateSearchInputV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, MigrationWorkflowV1, ProposalHostInvocationArtifact,
    SirEpisodeRunInput, SirIntentHypothesisSetProposalArtifact, SirTaskArtifactPath,
    SirTaskBundleV1, SirTaskLimits, SirTaskWorkspace, record_candidate_native_followup,
    record_candidate_native_repair, run_collection_candidate_episode,
    run_collection_candidate_native_followup_episode,
    run_collection_candidate_native_repair_episode, run_collection_candidate_revision_episode,
    run_sir_episode, validate_archived_candidate_build_diagnostic,
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

/// Runs any supported domain role through the same durable Proposal Host implementation.
///
/// # Errors
///
/// Rejects invalid role material or propagates the role-specific durable episode failure.
#[allow(clippy::too_many_lines)]
pub fn run_proposal_host_episode<E, C, T>(
    events: &mut E,
    content: &mut C,
    transport: &mut T,
    protocol_codec: NativeProtocolCodec,
    request: ProposalHostRequestV1,
) -> Result<ProposalHostTerminalV1, ProposalHostError>
where
    E: EventStore,
    C: ContentStore,
    T: ModelTransport,
{
    request.validate()?;
    let request_id = request.identity()?;
    let terminal_request = request.clone();
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
    let (publication, reason, steps_started) = match request.role {
        ProposalHostRoleRequestV1::Sir {
            task_id,
            recovery_request,
            ..
        } => {
            let outcome = run_sir_episode(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                SirEpisodeRunInput {
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
            let outcome = run_collection_candidate_episode(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateEpisodeRunInput {
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
            let outcome = run_collection_candidate_revision_episode(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateRevisionEpisodeRunInput {
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
            let outcome = run_collection_candidate_native_followup_episode(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateNativeFollowupEpisodeRunInput {
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
            let outcome = run_collection_candidate_native_repair_episode(
                events,
                content,
                transport,
                protocol_codec,
                workspace,
                CandidateNativeRepairEpisodeRunInput {
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
    let terminal = ProposalHostTerminalV1 {
        schema_version: schema_v1(),
        request: request_id,
        episode_id: runtime.episode_id,
        publication,
        completion_reason: reason,
        steps_started,
    };
    terminal.validate_against(&terminal_request)?;
    Ok(terminal)
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
    use super::ProposalHostBinaryIdentity;

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
}
