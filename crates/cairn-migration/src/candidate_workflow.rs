//! Controller-owned durable workflow spine for the Candidate native-build suffix.

use cairn_execution::{
    DockerImageId, ExecutionEnvironmentArtifact, ExecutionOutcome, ExecutionReceipt,
    ExecutionReceiptArtifact, InputBundleArtifact, JobContractArtifact,
};
use cairn_protocol::{
    AggregateId, AggregateKind, AssignmentId, AttemptId, CommandId, ContentId, ContentType,
    ControlMessageId, EpisodeId, EventId, JobId, LeaseId, ObservedAtUnixMillis, PlacementId,
    ReservationId, SchemaName, SchemaVersion, StreamRevision, TaskId,
};
use cairn_record::{
    EventEnvelope, EventStore, EventStoreError, ExpectedRevision, NewEvent, StreamId,
};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AdmittedCollectionOracleClaimArtifact, CandidateBuildEnvironmentProfileV1,
    CandidateNativeRepairParentV1, CollectionCandidateNativeBuildDiagnosticArtifact,
    CollectionCandidateNativeFollowupRevisionArtifact, CollectionCandidateNativeFollowupRevisionV1,
    CollectionCandidateNativeRepairBuildDiagnosticArtifact,
    CollectionCandidateNativeRepairRevisionArtifact, CollectionCandidateNativeRepairRevisionV1,
    CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
    CollectionCandidateSearchInputArtifact, CollectionCandidateSearchInputV1,
    CollectionOracleAdmissionPublicOutcomeArtifact, MigrationIntentContractArtifact,
};

const WORKFLOW_OPENED: &str = "migration.candidate-workflow-opened";
const NATIVE_BUILD_REQUESTED: &str = "migration.candidate-native-build-requested";
const NATIVE_BUILD_RECONCILIATION_REQUIRED: &str =
    "migration.candidate-native-build-reconciliation-required";
const NATIVE_BUILD_SUBJECT_FAILED: &str = "migration.candidate-native-build-subject-failed";
const CANDIDATE_EPISODE_REQUESTED: &str = "migration.candidate-episode-requested";
const CANDIDATE_PUBLICATION_RECORDED: &str = "migration.candidate-publication-recorded";
const WORKFLOW_TERMINATED: &str = "migration.candidate-workflow-terminated";

/// Exact admitted authority frozen before Candidate workflow execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateWorkflowAuthorityV1 {
    task_id: TaskId,
    candidate_search_input: ContentId<CollectionCandidateSearchInputArtifact>,
    admitted_intent: ContentId<MigrationIntentContractArtifact>,
    oracle_outcome: ContentId<CollectionOracleAdmissionPublicOutcomeArtifact>,
    oracle_claim: ContentId<AdmittedCollectionOracleClaimArtifact>,
}

impl CandidateWorkflowAuthorityV1 {
    /// Freezes the exact authority edges already carried by a validated Candidate search input.
    ///
    /// # Errors
    ///
    /// Rejects an identity that does not derive from the supplied canonical input.
    pub fn from_search_input(
        candidate_search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        input: &CollectionCandidateSearchInputV1,
    ) -> Result<Self, CandidateWorkflowError> {
        if input.identity().map_err(codec)? != candidate_search_input {
            return Err(CandidateWorkflowError::BindingMismatch);
        }
        Ok(Self {
            task_id: input.task_id(),
            candidate_search_input,
            admitted_intent: input.intent_contract(),
            oracle_outcome: input.oracle_outcome(),
            oracle_claim: input.oracle_claim(),
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn candidate_search_input(
        &self,
    ) -> ContentId<CollectionCandidateSearchInputArtifact> {
        self.candidate_search_input
    }

    #[must_use]
    pub const fn admitted_intent(&self) -> ContentId<MigrationIntentContractArtifact> {
        self.admitted_intent
    }

    #[must_use]
    pub const fn oracle_outcome(
        &self,
    ) -> ContentId<CollectionOracleAdmissionPublicOutcomeArtifact> {
        self.oracle_outcome
    }

    #[must_use]
    pub const fn oracle_claim(&self) -> ContentId<AdmittedCollectionOracleClaimArtifact> {
        self.oracle_claim
    }
}

/// Exact immutable Candidate publication selected for one native build.
///
/// A generic Candidate proposal identity cannot substitute for a revision publication.
///
/// ```compile_fail
/// use cairn_migration::{CandidateNativePublicationV1, CollectionCandidateProposalArtifact};
/// use cairn_protocol::ContentId;
/// fn invalid(id: ContentId<CollectionCandidateProposalArtifact>) {
///     let _ = CandidateNativePublicationV1::Revision(id);
/// }
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "identity")]
pub enum CandidateNativePublicationV1 {
    Revision(ContentId<CollectionCandidateRevisionArtifact>),
    NativeFollowup(ContentId<CollectionCandidateNativeFollowupRevisionArtifact>),
    NativeRepair(ContentId<CollectionCandidateNativeRepairRevisionArtifact>),
}

/// Exact diagnostic authority produced by a failed native build.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "identity")]
pub enum CandidateNativeDiagnosticV1 {
    NativeFollowup(ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>),
    NativeRepair(ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>),
}

/// Candidate episode requested by the workflow; this is the Proposal Host seam.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateEpisodeKindV1 {
    NativeFollowup,
    NativeRepair,
}

/// Positive maximum number of Candidate revision episodes authorized for this suffix.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CandidateRevisionRoundLimit(u32);

impl CandidateRevisionRoundLimit {
    /// Creates a positive revision-round budget.
    ///
    /// # Errors
    ///
    /// Rejects zero because it cannot authorize a revision episode.
    pub const fn new(value: u32) -> Result<Self, CandidateWorkflowError> {
        if value == 0 {
            Err(CandidateWorkflowError::InvalidRevisionBudget)
        } else {
            Ok(Self(value))
        }
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for CandidateRevisionRoundLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Number of Candidate revision episodes already committed by the workflow.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CandidateRevisionRoundCount(u32);

impl CandidateRevisionRoundCount {
    #[must_use]
    pub const fn zero() -> Self {
        Self(0)
    }

    fn increment(self) -> Result<Self, CandidateWorkflowError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(CandidateWorkflowError::InvalidHistory(
                "Candidate revision round count overflowed".into(),
            ))
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Scheduler identities durably allocated before any external build effect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateNativeBuildScheduleV1 {
    pub attempt_id: AttemptId,
    pub placement_id: PlacementId,
    pub reservation_id: ReservationId,
    pub assignment_id: AssignmentId,
    pub lease_id: LeaseId,
    pub offer_message_id: ControlMessageId,
    pub start_message_id: ControlMessageId,
    pub authorize_attempt_command: CommandId,
    pub reserve_placement_command: CommandId,
    pub grant_assignment_command: CommandId,
    pub enqueue_offer_command: CommandId,
}

/// Exact native-build effect authority returned after durable workflow recording.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateNativeBuildDispatchV1 {
    publication: CandidateNativePublicationV1,
    job_id: JobId,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
    schedule: CandidateNativeBuildScheduleV1,
}

impl CandidateNativeBuildDispatchV1 {
    #[must_use]
    pub const fn new(
        publication: CandidateNativePublicationV1,
        job_id: JobId,
        input_bundle: ContentId<InputBundleArtifact>,
        environment: ContentId<ExecutionEnvironmentArtifact>,
        contract: ContentId<JobContractArtifact>,
        schedule: CandidateNativeBuildScheduleV1,
    ) -> Self {
        Self {
            publication,
            job_id,
            input_bundle,
            environment,
            contract,
            schedule,
        }
    }

    #[must_use]
    pub const fn publication(&self) -> CandidateNativePublicationV1 {
        self.publication
    }
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }
    #[must_use]
    pub const fn input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle
    }
    #[must_use]
    pub const fn environment(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment
    }
    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }
    #[must_use]
    pub const fn schedule(&self) -> CandidateNativeBuildScheduleV1 {
        self.schedule
    }
}

/// Exact proposal request durably emitted for a Proposal Host or recorded consumer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEpisodeRequestV1 {
    kind: CandidateEpisodeKindV1,
    episode_id: EpisodeId,
    authority: CandidateWorkflowAuthorityV1,
    parent: CandidateNativePublicationV1,
    diagnostic: CandidateNativeDiagnosticV1,
    revision_round: CandidateRevisionRoundCount,
}

impl CandidateEpisodeRequestV1 {
    #[must_use]
    pub const fn kind(&self) -> CandidateEpisodeKindV1 {
        self.kind
    }
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }
    #[must_use]
    pub const fn authority(&self) -> &CandidateWorkflowAuthorityV1 {
        &self.authority
    }
    #[must_use]
    pub const fn parent(&self) -> CandidateNativePublicationV1 {
        self.parent
    }
    #[must_use]
    pub const fn diagnostic(&self) -> CandidateNativeDiagnosticV1 {
        self.diagnostic
    }
    #[must_use]
    pub const fn revision_round(&self) -> CandidateRevisionRoundCount {
        self.revision_round
    }
}

/// Why a subject failure did not open another Candidate revision episode.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateSubjectFailureStopV1 {
    RevisionBudgetExhausted,
}

/// Terminal outcome of this narrow suffix; never a migration verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum CandidateWorkflowTerminalV1 {
    NativeCompilationSucceeded {
        receipt: ContentId<ExecutionReceiptArtifact>,
    },
    NativeCompilationSubjectFailed {
        receipt: ContentId<ExecutionReceiptArtifact>,
        diagnostic: CandidateNativeDiagnosticV1,
        stop: CandidateSubjectFailureStopV1,
    },
    ExecutionInfrastructureFailed {
        receipt: ContentId<ExecutionReceiptArtifact>,
        outcome: ExecutionOutcome,
    },
    EvidenceIntegrityFailed {
        receipt: ContentId<ExecutionReceiptArtifact>,
    },
    Cancelled {
        receipt: ContentId<ExecutionReceiptArtifact>,
    },
}

/// Durable workflow state reconstructed solely from current-V1 events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateWorkflowStateV1 {
    NotFound,
    ReadyForNativeBuild {
        authority: CandidateWorkflowAuthorityV1,
        publication: CandidateNativePublicationV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
        revision_limit: CandidateRevisionRoundLimit,
        revisions_used: CandidateRevisionRoundCount,
    },
    NativeBuildRequested {
        authority: CandidateWorkflowAuthorityV1,
        dispatch: CandidateNativeBuildDispatchV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
        revision_limit: CandidateRevisionRoundLimit,
        revisions_used: CandidateRevisionRoundCount,
    },
    NativeBuildReconciliationRequired {
        authority: CandidateWorkflowAuthorityV1,
        dispatch: CandidateNativeBuildDispatchV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
        revision_limit: CandidateRevisionRoundLimit,
        revisions_used: CandidateRevisionRoundCount,
    },
    NativeBuildSubjectFailed {
        authority: CandidateWorkflowAuthorityV1,
        publication: CandidateNativePublicationV1,
        diagnostic: CandidateNativeDiagnosticV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
        revision_limit: CandidateRevisionRoundLimit,
        revisions_used: CandidateRevisionRoundCount,
    },
    CandidateEpisodeRequested {
        request: CandidateEpisodeRequestV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
        revision_limit: CandidateRevisionRoundLimit,
        revisions_used: CandidateRevisionRoundCount,
    },
    Terminal(CandidateWorkflowTerminalV1),
}

/// One exact next action selected from durable state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateWorkflowNextActionV1 {
    None,
    PrepareNativeBuild {
        publication: CandidateNativePublicationV1,
        image: DockerImageId,
        profile: CandidateBuildEnvironmentProfileV1,
    },
    ScheduleNativeBuild(CandidateNativeBuildDispatchV1),
    ReconcileNativeBuild(CandidateNativeBuildDispatchV1),
    PrepareCandidateEpisode {
        kind: CandidateEpisodeKindV1,
        authority: CandidateWorkflowAuthorityV1,
        parent: CandidateNativePublicationV1,
        diagnostic: CandidateNativeDiagnosticV1,
        revision_round: CandidateRevisionRoundCount,
    },
    RequestCandidateEpisode(CandidateEpisodeRequestV1),
    Terminal(CandidateWorkflowTerminalV1),
}

impl CandidateWorkflowStateV1 {
    /// Selects the one exact action implied by recovered durable state.
    ///
    /// # Errors
    ///
    /// Rejects a forged subject-failure state whose revision count cannot advance.
    pub fn next_action(&self) -> Result<CandidateWorkflowNextActionV1, CandidateWorkflowError> {
        Ok(match self {
            Self::NotFound => CandidateWorkflowNextActionV1::None,
            Self::ReadyForNativeBuild {
                publication,
                image,
                profile,
                ..
            } => CandidateWorkflowNextActionV1::PrepareNativeBuild {
                publication: *publication,
                image: image.clone(),
                profile: *profile,
            },
            Self::NativeBuildRequested { dispatch, .. } => {
                CandidateWorkflowNextActionV1::ScheduleNativeBuild(dispatch.clone())
            }
            Self::NativeBuildReconciliationRequired { dispatch, .. } => {
                CandidateWorkflowNextActionV1::ReconcileNativeBuild(dispatch.clone())
            }
            Self::NativeBuildSubjectFailed {
                authority,
                publication,
                diagnostic,
                revisions_used,
                ..
            } => CandidateWorkflowNextActionV1::PrepareCandidateEpisode {
                kind: candidate_episode_kind(*publication, *diagnostic)?,
                authority: authority.clone(),
                parent: *publication,
                diagnostic: *diagnostic,
                revision_round: revisions_used.increment()?,
            },
            Self::CandidateEpisodeRequested { request, .. } => {
                CandidateWorkflowNextActionV1::RequestCandidateEpisode(request.clone())
            }
            Self::Terminal(outcome) => CandidateWorkflowNextActionV1::Terminal(outcome.clone()),
        })
    }
}

/// Aggregate boundary for one CUDA-to-Ascend-C migration task workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationWorkflowV1 {
    task_id: TaskId,
    stream: StreamId,
}

impl MigrationWorkflowV1 {
    /// Creates the task-owned product aggregate and its private record-stream identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the current aggregate kind or task identity cannot be represented by
    /// the record boundary.
    pub fn new(task_id: TaskId) -> Result<Self, CandidateWorkflowError> {
        Ok(Self {
            task_id,
            stream: StreamId {
                kind: AggregateKind::new("migration-workflow")
                    .map_err(|error| invalid_history(error.to_string()))?,
                id: AggregateId::new(task_id.to_string())
                    .map_err(|error| invalid_history(error.to_string()))?,
            },
        })
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenedPayload {
    authority: CandidateWorkflowAuthorityV1,
    publication: CandidateNativePublicationV1,
    image: DockerImageId,
    profile: CandidateBuildEnvironmentProfileV1,
    revision_limit: CandidateRevisionRoundLimit,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildRequestedPayload {
    dispatch: CandidateNativeBuildDispatchV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReconciliationPayload {
    dispatch: CandidateNativeBuildDispatchV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectFailedPayload {
    dispatch: CandidateNativeBuildDispatchV1,
    receipt: ContentId<ExecutionReceiptArtifact>,
    diagnostic: CandidateNativeDiagnosticV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EpisodeRequestedPayload {
    request: CandidateEpisodeRequestV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationRecordedPayload {
    publication: CandidateNativePublicationV1,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerminatedPayload {
    dispatch: CandidateNativeBuildDispatchV1,
    outcome: CandidateWorkflowTerminalV1,
}

struct Projection {
    state: CandidateWorkflowStateV1,
    revision: Option<StreamRevision>,
    last_event_id: Option<EventId>,
    history: Vec<EventEnvelope>,
}

/// Opens the current-V1 workflow from an exact validated first Candidate revision.
///
/// # Errors
///
/// Rejects mismatched authority/publication identity, command conflict, an existing workflow, or
/// persistence/codec failure.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    reason = "each authority, policy, effect identity, and command field remains explicit"
)]
pub fn open_candidate_workflow<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    authority: CandidateWorkflowAuthorityV1,
    search_input: &CollectionCandidateSearchInputV1,
    revision: &CollectionCandidateRevisionV1,
    revision_id: ContentId<CollectionCandidateRevisionArtifact>,
    image: DockerImageId,
    profile: CandidateBuildEnvironmentProfileV1,
    revision_limit: CandidateRevisionRoundLimit,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    validate_content_id(revision_id, revision)?;
    if CandidateWorkflowAuthorityV1::from_search_input(
        authority.candidate_search_input,
        search_input,
    )? != authority
    {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    let projection = project(events, workflow)?;
    let payload = OpenedPayload {
        authority: authority.clone(),
        publication: CandidateNativePublicationV1::Revision(revision_id),
        image: image.clone(),
        profile,
        revision_limit,
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        WORKFLOW_OPENED,
        &payload,
    )? {
        return Ok(state);
    }
    if authority.task_id != workflow.task_id
        || revision.search_input() != authority.candidate_search_input
    {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    if projection.state != CandidateWorkflowStateV1::NotFound {
        return Err(CandidateWorkflowError::InvalidTransition);
    }
    append_transition(
        events,
        workflow,
        ExpectedRevision::NoStream,
        command_id,
        observed_at,
        WORKFLOW_OPENED,
        None,
        &payload,
    )?;
    recover_candidate_workflow(events, workflow)
}

/// Records all exact identities required to replay one native-build scheduling effect.
///
/// # Errors
///
/// Rejects publication drift, command conflict, an illegal state, or persistence/codec failure.
#[allow(
    clippy::needless_pass_by_value,
    reason = "the owned immutable dispatch becomes the durable event payload"
)]
pub fn request_candidate_native_build<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    dispatch: CandidateNativeBuildDispatchV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = BuildRequestedPayload {
        dispatch: dispatch.clone(),
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        NATIVE_BUILD_REQUESTED,
        &payload,
    )? {
        return Ok(state);
    }
    let CandidateWorkflowStateV1::ReadyForNativeBuild { publication, .. } = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    if *publication != dispatch.publication {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    append_transition(
        events,
        workflow,
        expected(&projection)?,
        command_id,
        observed_at,
        NATIVE_BUILD_REQUESTED,
        projection.last_event_id,
        &payload,
    )?;
    recover_candidate_workflow(events, workflow)
}

/// Marks an in-doubt build as reconcile-only; it cannot be replaced by a fresh build.
///
/// # Errors
///
/// Rejects changed dispatch identity, command conflict, an illegal state, or persistence failure.
#[allow(
    clippy::needless_pass_by_value,
    reason = "the owned immutable dispatch becomes the durable event payload"
)]
pub fn require_candidate_native_build_reconciliation<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    dispatch: CandidateNativeBuildDispatchV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    let projection = project(events, workflow)?;
    let payload = ReconciliationPayload {
        dispatch: dispatch.clone(),
    };
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        NATIVE_BUILD_RECONCILIATION_REQUIRED,
        &payload,
    )? {
        return Ok(state);
    }
    let CandidateWorkflowStateV1::NativeBuildRequested {
        dispatch: active, ..
    } = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    if active != &dispatch {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    append_transition(
        events,
        workflow,
        expected(&projection)?,
        command_id,
        observed_at,
        NATIVE_BUILD_RECONCILIATION_REQUIRED,
        projection.last_event_id,
        &payload,
    )?;
    recover_candidate_workflow(events, workflow)
}

/// Records a verified subject failure and either exposes a repair step or stops at budget.
///
/// # Errors
///
/// Rejects receipt/diagnostic drift, command conflict, an illegal state, or persistence failure.
pub fn record_candidate_native_subject_failure<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    diagnostic: CandidateNativeDiagnosticV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    let projection = project(events, workflow)?;
    if let Some(prior) = prior_command(&projection, command_id) {
        validate_replay_header(prior, observed_at)?;
        match prior.schema_name.as_str() {
            NATIVE_BUILD_SUBJECT_FAILED => {
                let payload: SubjectFailedPayload = decode(&prior.payload)?;
                if payload.receipt != receipt_id || payload.diagnostic != diagnostic {
                    return Err(CandidateWorkflowError::CommandConflict);
                }
                validate_receipt(
                    &payload.dispatch,
                    receipt_id,
                    receipt,
                    ExecutionOutcome::SubjectFailed,
                )?;
            }
            WORKFLOW_TERMINATED => {
                let payload: TerminatedPayload = decode(&prior.payload)?;
                let expected = CandidateWorkflowTerminalV1::NativeCompilationSubjectFailed {
                    receipt: receipt_id,
                    diagnostic,
                    stop: CandidateSubjectFailureStopV1::RevisionBudgetExhausted,
                };
                if payload.outcome != expected {
                    return Err(CandidateWorkflowError::CommandConflict);
                }
                validate_receipt(
                    &payload.dispatch,
                    receipt_id,
                    receipt,
                    ExecutionOutcome::SubjectFailed,
                )?;
            }
            _ => return Err(CandidateWorkflowError::CommandConflict),
        }
        return Ok(projection.state);
    };
    let (CandidateWorkflowStateV1::NativeBuildRequested {
        dispatch,
        revision_limit,
        revisions_used,
        ..
    }
    | CandidateWorkflowStateV1::NativeBuildReconciliationRequired {
        dispatch,
        revision_limit,
        revisions_used,
        ..
    }) = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    validate_receipt(
        dispatch,
        receipt_id,
        receipt,
        ExecutionOutcome::SubjectFailed,
    )?;
    validate_diagnostic_kind(dispatch.publication, diagnostic)?;
    if revisions_used.get() >= revision_limit.get() {
        let outcome = CandidateWorkflowTerminalV1::NativeCompilationSubjectFailed {
            receipt: receipt_id,
            diagnostic,
            stop: CandidateSubjectFailureStopV1::RevisionBudgetExhausted,
        };
        append_transition(
            events,
            workflow,
            expected(&projection)?,
            command_id,
            observed_at,
            WORKFLOW_TERMINATED,
            projection.last_event_id,
            &TerminatedPayload {
                dispatch: dispatch.clone(),
                outcome,
            },
        )?;
    } else {
        append_transition(
            events,
            workflow,
            expected(&projection)?,
            command_id,
            observed_at,
            NATIVE_BUILD_SUBJECT_FAILED,
            projection.last_event_id,
            &SubjectFailedPayload {
                dispatch: dispatch.clone(),
                receipt: receipt_id,
                diagnostic,
            },
        )?;
    }
    recover_candidate_workflow(events, workflow)
}

/// Durably allocates one exact Candidate episode request before any model effect.
///
/// # Errors
///
/// Rejects a reused command with changed input, an illegal state, inconsistent authority, or
/// persistence failure.
pub fn request_candidate_episode<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    episode_id: EpisodeId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    let projection = project(events, workflow)?;
    if let Some(prior) = prior_command(&projection, command_id) {
        validate_replay_header(prior, observed_at)?;
        if prior.schema_name.as_str() != CANDIDATE_EPISODE_REQUESTED {
            return Err(CandidateWorkflowError::CommandConflict);
        }
        let payload: EpisodeRequestedPayload = decode(&prior.payload)?;
        if payload.request.episode_id != episode_id {
            return Err(CandidateWorkflowError::CommandConflict);
        }
        return Ok(projection.state);
    }
    let CandidateWorkflowStateV1::NativeBuildSubjectFailed {
        authority,
        publication,
        diagnostic,
        revisions_used,
        ..
    } = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    let kind = candidate_episode_kind(*publication, *diagnostic)?;
    let request = CandidateEpisodeRequestV1 {
        kind,
        episode_id,
        authority: authority.clone(),
        parent: *publication,
        diagnostic: *diagnostic,
        revision_round: revisions_used.increment()?,
    };
    append_transition(
        events,
        workflow,
        expected(&projection)?,
        command_id,
        observed_at,
        CANDIDATE_EPISODE_REQUESTED,
        projection.last_event_id,
        &EpisodeRequestedPayload { request },
    )?;
    recover_candidate_workflow(events, workflow)
}

/// Records a validated native-follow-up publication returned for the exact request.
///
/// # Errors
///
/// Rejects publication identity or request binding drift, command conflict, an illegal state, or
/// persistence failure.
pub fn record_candidate_native_followup<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    followup: &CollectionCandidateNativeFollowupRevisionV1,
    followup_id: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    validate_content_id(followup_id, followup)?;
    let projection = project(events, workflow)?;
    let publication = CandidateNativePublicationV1::NativeFollowup(followup_id);
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PUBLICATION_RECORDED,
        &PublicationRecordedPayload { publication },
    )? {
        return Ok(state);
    }
    let CandidateWorkflowStateV1::CandidateEpisodeRequested { request, .. } = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    let (
        CandidateEpisodeKindV1::NativeFollowup,
        CandidateNativePublicationV1::Revision(parent),
        CandidateNativeDiagnosticV1::NativeFollowup(diagnostic),
    ) = (request.kind, request.parent, request.diagnostic)
    else {
        return Err(CandidateWorkflowError::BindingMismatch);
    };
    if followup.search_input() != request.authority.candidate_search_input
        || followup.previous_revision() != parent
        || followup.build_diagnostic() != diagnostic
        || followup.episode_id() != request.episode_id
    {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    record_publication(
        events,
        workflow,
        &projection,
        publication,
        command_id,
        observed_at,
    )
}

/// Records a validated native-repair publication returned for the exact request.
///
/// # Errors
///
/// Rejects publication identity or repair-lineage drift, command conflict, an illegal state, or
/// persistence failure.
pub fn record_candidate_native_repair<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    repair: &CollectionCandidateNativeRepairRevisionV1,
    repair_id: ContentId<CollectionCandidateNativeRepairRevisionArtifact>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    validate_content_id(repair_id, repair)?;
    let projection = project(events, workflow)?;
    let publication = CandidateNativePublicationV1::NativeRepair(repair_id);
    if let Some(state) = exact_replay(
        &projection,
        command_id,
        observed_at,
        CANDIDATE_PUBLICATION_RECORDED,
        &PublicationRecordedPayload { publication },
    )? {
        return Ok(state);
    }
    let CandidateWorkflowStateV1::CandidateEpisodeRequested { request, .. } = &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    let CandidateEpisodeKindV1::NativeRepair = request.kind else {
        return Err(CandidateWorkflowError::BindingMismatch);
    };
    let CandidateNativeDiagnosticV1::NativeRepair(diagnostic) = request.diagnostic else {
        return Err(CandidateWorkflowError::BindingMismatch);
    };
    let expected_parent = match request.parent {
        CandidateNativePublicationV1::NativeFollowup(id) => {
            CandidateNativeRepairParentV1::RootFollowup(id)
        }
        CandidateNativePublicationV1::NativeRepair(id) => CandidateNativeRepairParentV1::Repair(id),
        CandidateNativePublicationV1::Revision(_) => {
            return Err(CandidateWorkflowError::BindingMismatch);
        }
    };
    if repair.search_input() != request.authority.candidate_search_input
        || repair.parent() != expected_parent
        || repair.build_diagnostic() != diagnostic
        || repair.episode_id() != request.episode_id
    {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    record_publication(
        events,
        workflow,
        &projection,
        publication,
        command_id,
        observed_at,
    )
}

/// Records a verified terminal execution receipt other than `SubjectFailed`.
///
/// # Errors
///
/// Rejects receipt/dispatch drift, subject failure on this command, command conflict, an illegal
/// state, or persistence failure.
pub fn record_candidate_native_terminal<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    validate_content_id(receipt_id, receipt)?;
    let outcome = match receipt.outcome() {
        ExecutionOutcome::Succeeded => CandidateWorkflowTerminalV1::NativeCompilationSucceeded {
            receipt: receipt_id,
        },
        ExecutionOutcome::SubjectFailed => return Err(CandidateWorkflowError::InvalidTransition),
        ExecutionOutcome::Cancelled => CandidateWorkflowTerminalV1::Cancelled {
            receipt: receipt_id,
        },
        ExecutionOutcome::IntegrityViolation => {
            CandidateWorkflowTerminalV1::EvidenceIntegrityFailed {
                receipt: receipt_id,
            }
        }
        outcome @ (ExecutionOutcome::TimedOut | ExecutionOutcome::InfrastructureFailed) => {
            CandidateWorkflowTerminalV1::ExecutionInfrastructureFailed {
                receipt: receipt_id,
                outcome,
            }
        }
    };
    let projection = project(events, workflow)?;
    if prior_command(&projection, command_id).is_some() {
        let dispatch = dispatch_before_command(&projection, command_id)?;
        validate_receipt_binding(&dispatch, receipt_id, receipt)?;
        let payload = TerminatedPayload { dispatch, outcome };
        return exact_replay(
            &projection,
            command_id,
            observed_at,
            WORKFLOW_TERMINATED,
            &payload,
        )?
        .ok_or(CandidateWorkflowError::CommandConflict);
    }
    let (CandidateWorkflowStateV1::NativeBuildRequested { dispatch, .. }
    | CandidateWorkflowStateV1::NativeBuildReconciliationRequired { dispatch, .. }) =
        &projection.state
    else {
        return Err(CandidateWorkflowError::InvalidTransition);
    };
    validate_receipt_binding(dispatch, receipt_id, receipt)?;
    let payload = TerminatedPayload {
        dispatch: dispatch.clone(),
        outcome,
    };
    append_transition(
        events,
        workflow,
        expected(&projection)?,
        command_id,
        observed_at,
        WORKFLOW_TERMINATED,
        projection.last_event_id,
        &payload,
    )?;
    recover_candidate_workflow(events, workflow)
}

/// Reconstructs the exact current state and rejects every non-V1 or illegal history.
///
/// # Errors
///
/// Rejects noncanonical/non-V1 payloads, illegal transitions, invalid semantic combinations, or
/// storage failure.
pub fn recover_candidate_workflow<E: EventStore>(
    events: &E,
    workflow: &MigrationWorkflowV1,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    Ok(project(events, workflow)?.state)
}

fn record_publication<E: EventStore>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    projection: &Projection,
    publication: CandidateNativePublicationV1,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    append_transition(
        events,
        workflow,
        expected(projection)?,
        command_id,
        observed_at,
        CANDIDATE_PUBLICATION_RECORDED,
        projection.last_event_id,
        &PublicationRecordedPayload { publication },
    )?;
    recover_candidate_workflow(events, workflow)
}

fn validate_receipt(
    dispatch: &CandidateNativeBuildDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
    expected_outcome: ExecutionOutcome,
) -> Result<(), CandidateWorkflowError> {
    validate_receipt_binding(dispatch, receipt_id, receipt)?;
    if receipt.outcome() != expected_outcome {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    Ok(())
}

fn validate_content_id<T: ContentType, V: Serialize>(
    expected: ContentId<T>,
    value: &V,
) -> Result<(), CandidateWorkflowError> {
    let bytes = cairn_codec::to_vec(value).map_err(codec)?;
    if ContentId::derive(&bytes).map_err(codec)? != expected {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    Ok(())
}

fn validate_receipt_binding(
    dispatch: &CandidateNativeBuildDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: &ExecutionReceipt,
) -> Result<(), CandidateWorkflowError> {
    validate_content_id(receipt_id, receipt)?;
    if receipt.job_id() != dispatch.job_id
        || receipt.attempt_id() != dispatch.schedule.attempt_id
        || receipt.contract_id() != dispatch.contract
    {
        return Err(CandidateWorkflowError::BindingMismatch);
    }
    Ok(())
}

fn validate_diagnostic_kind(
    publication: CandidateNativePublicationV1,
    diagnostic: CandidateNativeDiagnosticV1,
) -> Result<(), CandidateWorkflowError> {
    match (publication, diagnostic) {
        (
            CandidateNativePublicationV1::Revision(_),
            CandidateNativeDiagnosticV1::NativeFollowup(_),
        )
        | (
            CandidateNativePublicationV1::NativeFollowup(_)
            | CandidateNativePublicationV1::NativeRepair(_),
            CandidateNativeDiagnosticV1::NativeRepair(_),
        ) => Ok(()),
        _ => Err(CandidateWorkflowError::BindingMismatch),
    }
}

fn candidate_episode_kind(
    publication: CandidateNativePublicationV1,
    diagnostic: CandidateNativeDiagnosticV1,
) -> Result<CandidateEpisodeKindV1, CandidateWorkflowError> {
    match (publication, diagnostic) {
        (
            CandidateNativePublicationV1::Revision(_),
            CandidateNativeDiagnosticV1::NativeFollowup(_),
        ) => Ok(CandidateEpisodeKindV1::NativeFollowup),
        (
            CandidateNativePublicationV1::NativeFollowup(_)
            | CandidateNativePublicationV1::NativeRepair(_),
            CandidateNativeDiagnosticV1::NativeRepair(_),
        ) => Ok(CandidateEpisodeKindV1::NativeRepair),
        _ => Err(CandidateWorkflowError::BindingMismatch),
    }
}

fn validate_terminal_outcome(
    outcome: &CandidateWorkflowTerminalV1,
    dispatch: &CandidateNativeBuildDispatchV1,
    revision_limit: CandidateRevisionRoundLimit,
    revisions_used: CandidateRevisionRoundCount,
) -> Result<(), CandidateWorkflowError> {
    match outcome {
        CandidateWorkflowTerminalV1::ExecutionInfrastructureFailed { outcome, .. }
            if !matches!(
                outcome,
                ExecutionOutcome::TimedOut | ExecutionOutcome::InfrastructureFailed
            ) =>
        {
            Err(invalid_history(
                "terminal infrastructure classification changed",
            ))
        }
        CandidateWorkflowTerminalV1::NativeCompilationSubjectFailed { diagnostic, .. } => {
            validate_diagnostic_kind(dispatch.publication, *diagnostic)?;
            if revisions_used.get() < revision_limit.get() {
                return Err(invalid_history(
                    "subject failure terminated before its revision budget was exhausted",
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn project<E: EventStore>(
    events: &E,
    workflow: &MigrationWorkflowV1,
) -> Result<Projection, CandidateWorkflowError> {
    let history = events.read_stream(&workflow.stream, None)?;
    let mut state = CandidateWorkflowStateV1::NotFound;
    let mut parent_event_id = None;
    for event in &history {
        if event.schema_version != schema_v1() {
            return Err(invalid_history("non-V1 workflow event"));
        }
        if event.parent_event_id != parent_event_id {
            return Err(invalid_history("workflow event causal parent changed"));
        }
        state = apply(
            workflow.task_id,
            state,
            event.schema_name.as_str(),
            &event.payload,
        )?;
        parent_event_id = Some(event.event_id);
    }
    let last = history.last();
    Ok(Projection {
        state,
        revision: last
            .map(|event| StreamRevision::new(event.sequence.get()))
            .transpose()
            .map_err(|error| invalid_history(error.to_string()))?,
        last_event_id: last.map(|event| event.event_id),
        history,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the event fold keeps all legal current-V1 transitions visible in one exhaustive match"
)]
fn apply(
    task_id: TaskId,
    state: CandidateWorkflowStateV1,
    schema: &str,
    bytes: &[u8],
) -> Result<CandidateWorkflowStateV1, CandidateWorkflowError> {
    match (state, schema) {
        (CandidateWorkflowStateV1::NotFound, WORKFLOW_OPENED) => {
            let payload: OpenedPayload = decode(bytes)?;
            if payload.authority.task_id != task_id
                || !matches!(
                    payload.publication,
                    CandidateNativePublicationV1::Revision(_)
                )
            {
                return Err(invalid_history("workflow opening authority changed"));
            }
            Ok(CandidateWorkflowStateV1::ReadyForNativeBuild {
                authority: payload.authority,
                publication: payload.publication,
                image: payload.image,
                profile: payload.profile,
                revision_limit: payload.revision_limit,
                revisions_used: CandidateRevisionRoundCount::zero(),
            })
        }
        (
            CandidateWorkflowStateV1::ReadyForNativeBuild {
                authority,
                publication,
                image,
                profile,
                revision_limit,
                revisions_used,
            },
            NATIVE_BUILD_REQUESTED,
        ) => {
            let payload: BuildRequestedPayload = decode(bytes)?;
            if payload.dispatch.publication != publication {
                return Err(invalid_history("build publication changed"));
            }
            Ok(CandidateWorkflowStateV1::NativeBuildRequested {
                authority,
                dispatch: payload.dispatch,
                image,
                profile,
                revision_limit,
                revisions_used,
            })
        }
        (
            CandidateWorkflowStateV1::NativeBuildRequested {
                authority,
                dispatch,
                image,
                profile,
                revision_limit,
                revisions_used,
            },
            NATIVE_BUILD_RECONCILIATION_REQUIRED,
        ) => {
            let payload: ReconciliationPayload = decode(bytes)?;
            if payload.dispatch != dispatch {
                return Err(invalid_history("reconcile dispatch changed"));
            }
            Ok(
                CandidateWorkflowStateV1::NativeBuildReconciliationRequired {
                    authority,
                    dispatch,
                    image,
                    profile,
                    revision_limit,
                    revisions_used,
                },
            )
        }
        (
            CandidateWorkflowStateV1::NativeBuildRequested {
                authority,
                dispatch,
                image,
                profile,
                revision_limit,
                revisions_used,
            }
            | CandidateWorkflowStateV1::NativeBuildReconciliationRequired {
                authority,
                dispatch,
                image,
                profile,
                revision_limit,
                revisions_used,
            },
            NATIVE_BUILD_SUBJECT_FAILED,
        ) => {
            let payload: SubjectFailedPayload = decode(bytes)?;
            if payload.dispatch != dispatch {
                return Err(invalid_history("failed dispatch changed"));
            }
            validate_diagnostic_kind(dispatch.publication, payload.diagnostic)?;
            if revisions_used.get() >= revision_limit.get() {
                return Err(invalid_history(
                    "subject failure continued after its revision budget was exhausted",
                ));
            }
            Ok(CandidateWorkflowStateV1::NativeBuildSubjectFailed {
                authority,
                publication: dispatch.publication,
                diagnostic: payload.diagnostic,
                image,
                profile,
                revision_limit,
                revisions_used,
            })
        }
        (
            CandidateWorkflowStateV1::NativeBuildSubjectFailed {
                authority,
                publication,
                diagnostic,
                image,
                profile,
                revision_limit,
                revisions_used,
            },
            CANDIDATE_EPISODE_REQUESTED,
        ) => {
            let payload: EpisodeRequestedPayload = decode(bytes)?;
            if payload.request.authority != authority
                || payload.request.parent != publication
                || payload.request.diagnostic != diagnostic
                || payload.request.kind != candidate_episode_kind(publication, diagnostic)?
                || payload.request.revision_round != revisions_used.increment()?
            {
                return Err(invalid_history(
                    "Candidate episode request changed authority",
                ));
            }
            Ok(CandidateWorkflowStateV1::CandidateEpisodeRequested {
                request: payload.request,
                image,
                profile,
                revision_limit,
                revisions_used,
            })
        }
        (
            CandidateWorkflowStateV1::CandidateEpisodeRequested {
                request,
                image,
                profile,
                revision_limit,
                ..
            },
            CANDIDATE_PUBLICATION_RECORDED,
        ) => {
            let payload: PublicationRecordedPayload = decode(bytes)?;
            let valid_kind = matches!(
                (request.kind, payload.publication),
                (
                    CandidateEpisodeKindV1::NativeFollowup,
                    CandidateNativePublicationV1::NativeFollowup(_)
                ) | (
                    CandidateEpisodeKindV1::NativeRepair,
                    CandidateNativePublicationV1::NativeRepair(_)
                )
            );
            if !valid_kind {
                return Err(invalid_history("Candidate publication kind changed"));
            }
            Ok(CandidateWorkflowStateV1::ReadyForNativeBuild {
                authority: request.authority,
                publication: payload.publication,
                image,
                profile,
                revision_limit,
                revisions_used: request.revision_round,
            })
        }
        (
            CandidateWorkflowStateV1::NativeBuildRequested {
                dispatch,
                revision_limit,
                revisions_used,
                ..
            }
            | CandidateWorkflowStateV1::NativeBuildReconciliationRequired {
                dispatch,
                revision_limit,
                revisions_used,
                ..
            },
            WORKFLOW_TERMINATED,
        ) => {
            let payload: TerminatedPayload = decode(bytes)?;
            if payload.dispatch != dispatch {
                return Err(invalid_history("terminal dispatch changed"));
            }
            validate_terminal_outcome(&payload.outcome, &dispatch, revision_limit, revisions_used)?;
            Ok(CandidateWorkflowStateV1::Terminal(payload.outcome))
        }
        (_, _) => Err(invalid_history(
            "illegal Candidate workflow event transition",
        )),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "record envelope authority is intentionally explicit at this private boundary"
)]
fn append_transition<E: EventStore, P: Serialize>(
    events: &mut E,
    workflow: &MigrationWorkflowV1,
    expected: ExpectedRevision,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    parent_event_id: Option<EventId>,
    payload: &P,
) -> Result<(), CandidateWorkflowError> {
    let event = NewEvent {
        schema_name: SchemaName::new(schema).map_err(|error| invalid_history(error.to_string()))?,
        schema_version: schema_v1(),
        parent_event_id,
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload).map_err(codec)?,
    };
    events.append(&workflow.stream, expected, command_id, &[event])?;
    Ok(())
}

fn exact_replay<P: Serialize>(
    projection: &Projection,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
    schema: &str,
    payload: &P,
) -> Result<Option<CandidateWorkflowStateV1>, CandidateWorkflowError> {
    let Some(prior) = projection
        .history
        .iter()
        .find(|event| event.command_id == *command_id)
    else {
        return Ok(None);
    };
    let payload = cairn_codec::to_vec(payload).map_err(codec)?;
    if prior.schema_name.as_str() == schema
        && prior.schema_version == schema_v1()
        && prior.observed_at_unix_ms == observed_at.get()
        && prior.payload == payload
    {
        Ok(Some(projection.state.clone()))
    } else {
        Err(CandidateWorkflowError::CommandConflict)
    }
}

fn prior_command<'a>(
    projection: &'a Projection,
    command_id: &CommandId,
) -> Option<&'a EventEnvelope> {
    projection
        .history
        .iter()
        .find(|event| event.command_id == *command_id)
}

fn validate_replay_header(
    prior: &EventEnvelope,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), CandidateWorkflowError> {
    if prior.schema_version != schema_v1() || prior.observed_at_unix_ms != observed_at.get() {
        return Err(CandidateWorkflowError::CommandConflict);
    }
    Ok(())
}

fn dispatch_before_command(
    projection: &Projection,
    command_id: &CommandId,
) -> Result<CandidateNativeBuildDispatchV1, CandidateWorkflowError> {
    let command_index = projection
        .history
        .iter()
        .position(|event| event.command_id == *command_id)
        .ok_or(CandidateWorkflowError::CommandConflict)?;
    let event = projection.history[..command_index]
        .iter()
        .rev()
        .find(|event| event.schema_name.as_str() == NATIVE_BUILD_REQUESTED)
        .ok_or_else(|| invalid_history("terminal command has no preceding native build"))?;
    Ok(decode::<BuildRequestedPayload>(&event.payload)?.dispatch)
}

fn expected(projection: &Projection) -> Result<ExpectedRevision, CandidateWorkflowError> {
    projection
        .revision
        .map(ExpectedRevision::Exact)
        .ok_or(CandidateWorkflowError::InvalidTransition)
}

fn decode<T: for<'de> Deserialize<'de> + Serialize>(
    bytes: &[u8],
) -> Result<T, CandidateWorkflowError> {
    let value = cairn_codec::from_slice(bytes).map_err(codec)?;
    if cairn_codec::to_vec(&value).map_err(codec)? != bytes {
        return Err(invalid_history("noncanonical workflow event payload"));
    }
    Ok(value)
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("schema version one is valid")
}

fn codec(error: impl std::fmt::Display) -> CandidateWorkflowError {
    CandidateWorkflowError::Codec(error.to_string())
}

fn invalid_history(message: impl Into<String>) -> CandidateWorkflowError {
    CandidateWorkflowError::InvalidHistory(message.into())
}

#[derive(Debug, Error)]
pub enum CandidateWorkflowError {
    #[error("Candidate workflow transition is illegal from the current state")]
    InvalidTransition,
    #[error("Candidate workflow authority or exact artifact binding changed")]
    BindingMismatch,
    #[error("Candidate workflow command was already used with different input")]
    CommandConflict,
    #[error("Candidate revision budget must be positive")]
    InvalidRevisionBudget,
    #[error("invalid Candidate workflow history: {0}")]
    InvalidHistory(String),
    #[error("Candidate workflow codec failed: {0}")]
    Codec(String),
    #[error(transparent)]
    Event(#[from] EventStoreError),
}

#[cfg(test)]
mod tests {
    use cairn_execution::{
        ArchivedOutput, ExecutionElapsedMillis, ExecutionEvidenceArtifact, ExecutionStderrArtifact,
        ExecutionStdoutArtifact,
    };
    use cairn_store_sqlite::SqliteEventStore;
    use serde::Serialize;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        CollectionCandidateBuildDiagnosticArtifact, CollectionCandidateExplanation,
        CollectionCandidateProposalArtifact, CollectionCandidateProposalSubmissionV1,
        CollectionCandidateSearchAuthorityInput, CollectionCandidateSourceFileV1,
        CollectionCandidateSourcePath, CollectionCandidateSourceText,
        CollectionOracleClaimDomainV1, CollectionOracleClaimStrengthV1, SirCallerClaimId,
        SirResolvedRuntimeModelArtifact, prepare_collection_candidate_search_input,
    };

    #[derive(Serialize)]
    struct RevisionWire {
        schema_version: u16,
        search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        parent_proposal: ContentId<CollectionCandidateProposalArtifact>,
        build_diagnostic: ContentId<CollectionCandidateBuildDiagnosticArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
        submission: CollectionCandidateProposalSubmissionV1,
    }

    #[derive(Serialize)]
    struct FollowupWire {
        schema_version: u16,
        search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        previous_revision: ContentId<CollectionCandidateRevisionArtifact>,
        build_diagnostic: ContentId<CollectionCandidateNativeBuildDiagnosticArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
        submission: CollectionCandidateProposalSubmissionV1,
    }

    #[derive(Serialize)]
    struct RepairWire {
        schema_version: u16,
        search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        root_followup: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
        parent: CandidateNativeRepairParentV1,
        build_diagnostic: ContentId<CollectionCandidateNativeRepairBuildDiagnosticArtifact>,
        episode_id: EpisodeId,
        model_configuration: ContentId<SirResolvedRuntimeModelArtifact>,
        submission: CollectionCandidateProposalSubmissionV1,
    }

    #[derive(Serialize)]
    struct ReceiptWire {
        schema_version: u16,
        job_id: JobId,
        attempt_id: AttemptId,
        contract_id: ContentId<JobContractArtifact>,
        outcome: ExecutionOutcome,
        exit_code: Option<i32>,
        elapsed_ms: ExecutionElapsedMillis,
        stdout_id: ContentId<ExecutionStdoutArtifact>,
        stderr_id: ContentId<ExecutionStderrArtifact>,
        evidence_id: ContentId<ExecutionEvidenceArtifact>,
        outputs: Vec<ArchivedOutput>,
    }

    fn id<T: ContentType>(label: impl AsRef<[u8]>) -> ContentId<T> {
        ContentId::derive(label.as_ref()).expect("test content identity")
    }

    fn submission(label: &str, round: &str) -> CollectionCandidateProposalSubmissionV1 {
        #[derive(Serialize)]
        struct SubmissionWire {
            schema_version: u16,
            files: Vec<CollectionCandidateSourceFileV1>,
            primary_source: CollectionCandidateSourcePath,
            explanation: CollectionCandidateExplanation,
        }
        #[derive(Serialize)]
        struct FileWire {
            path: CollectionCandidateSourcePath,
            source: CollectionCandidateSourceText,
        }
        let file_bytes = cairn_codec::to_vec(&FileWire {
            path: CollectionCandidateSourcePath::new("operator.asc").expect("path"),
            source: CollectionCandidateSourceText::new(format!(
                "// material {label}; recorded round {round}\nvoid kernel() {{}}\n"
            ))
            .expect("source"),
        })
        .expect("file bytes");
        let file = cairn_codec::from_slice(&file_bytes).expect("source file");
        let bytes = cairn_codec::to_vec(&SubmissionWire {
            schema_version: 1,
            files: vec![file],
            primary_source: CollectionCandidateSourcePath::new("operator.asc").expect("path"),
            explanation: CollectionCandidateExplanation::new(format!(
                "Recorded generic consumer material {label}, round {round}."
            ))
            .expect("explanation"),
        })
        .expect("submission bytes");
        cairn_codec::from_slice(&bytes).expect("submission")
    }

    fn revision(
        search_input: ContentId<CollectionCandidateSearchInputArtifact>,
        label: &str,
    ) -> CollectionCandidateRevisionV1 {
        let bytes = cairn_codec::to_vec(&RevisionWire {
            schema_version: 1,
            search_input,
            parent_proposal: id(format!("{label}-proposal")),
            build_diagnostic: id(format!("{label}-generic-diagnostic")),
            episode_id: EpisodeId::new(),
            model_configuration: id(format!("{label}-model-config")),
            submission: submission(label, "revision"),
        })
        .expect("revision bytes");
        cairn_codec::from_slice(&bytes).expect("revision")
    }

    fn recorded_followup(
        request: &CandidateEpisodeRequestV1,
        label: &str,
    ) -> CollectionCandidateNativeFollowupRevisionV1 {
        let CandidateNativePublicationV1::Revision(previous_revision) = request.parent() else {
            panic!("recorded follow-up requires revision parent");
        };
        let CandidateNativeDiagnosticV1::NativeFollowup(build_diagnostic) = request.diagnostic()
        else {
            panic!("recorded follow-up requires native follow-up diagnostic");
        };
        let bytes = cairn_codec::to_vec(&FollowupWire {
            schema_version: 1,
            search_input: request.authority().candidate_search_input(),
            previous_revision,
            build_diagnostic,
            episode_id: request.episode_id(),
            model_configuration: id(format!("{label}-followup-model-config")),
            submission: submission(label, "followup"),
        })
        .expect("follow-up bytes");
        cairn_codec::from_slice(&bytes).expect("follow-up")
    }

    fn recorded_repair(
        request: &CandidateEpisodeRequestV1,
        root_followup: ContentId<CollectionCandidateNativeFollowupRevisionArtifact>,
        label: &str,
    ) -> CollectionCandidateNativeRepairRevisionV1 {
        let parent = match request.parent() {
            CandidateNativePublicationV1::NativeFollowup(id) => {
                CandidateNativeRepairParentV1::RootFollowup(id)
            }
            CandidateNativePublicationV1::NativeRepair(id) => {
                CandidateNativeRepairParentV1::Repair(id)
            }
            CandidateNativePublicationV1::Revision(_) => panic!("repair parent must be native"),
        };
        let CandidateNativeDiagnosticV1::NativeRepair(build_diagnostic) = request.diagnostic()
        else {
            panic!("recorded repair requires repair diagnostic");
        };
        let bytes = cairn_codec::to_vec(&RepairWire {
            schema_version: 1,
            search_input: request.authority().candidate_search_input(),
            root_followup,
            parent,
            build_diagnostic,
            episode_id: request.episode_id(),
            model_configuration: id(format!("{label}-repair-model-config")),
            submission: submission(label, "repair"),
        })
        .expect("repair bytes");
        cairn_codec::from_slice(&bytes).expect("repair")
    }

    fn schedule() -> CandidateNativeBuildScheduleV1 {
        CandidateNativeBuildScheduleV1 {
            attempt_id: AttemptId::new(),
            placement_id: PlacementId::new(),
            reservation_id: ReservationId::new(),
            assignment_id: AssignmentId::new(),
            lease_id: LeaseId::new(),
            offer_message_id: ControlMessageId::new(),
            start_message_id: ControlMessageId::new(),
            authorize_attempt_command: CommandId::new(),
            reserve_placement_command: CommandId::new(),
            grant_assignment_command: CommandId::new(),
            enqueue_offer_command: CommandId::new(),
        }
    }

    fn dispatch(publication: CandidateNativePublicationV1) -> CandidateNativeBuildDispatchV1 {
        CandidateNativeBuildDispatchV1::new(
            publication,
            JobId::new(),
            id(b"recorded input bundle"),
            id(b"recorded environment"),
            id(b"recorded contract"),
            schedule(),
        )
    }

    fn receipt(
        dispatch: &CandidateNativeBuildDispatchV1,
        outcome: ExecutionOutcome,
        label: &str,
    ) -> (ExecutionReceipt, ContentId<ExecutionReceiptArtifact>) {
        let wire = ReceiptWire {
            schema_version: 1,
            job_id: dispatch.job_id(),
            attempt_id: dispatch.schedule().attempt_id,
            contract_id: dispatch.contract(),
            outcome,
            exit_code: Some(i32::from(outcome != ExecutionOutcome::Succeeded)),
            elapsed_ms: ExecutionElapsedMillis::new(7),
            stdout_id: id(format!("{label}-stdout")),
            stderr_id: id(format!("{label}-stderr")),
            evidence_id: id(format!("{label}-evidence")),
            outputs: Vec::new(),
        };
        let bytes = cairn_codec::to_vec(&wire).expect("receipt bytes");
        let receipt = cairn_codec::from_slice(&bytes).expect("receipt");
        (receipt, id(bytes))
    }

    fn authority(
        task_id: TaskId,
        label: &str,
    ) -> (
        CandidateWorkflowAuthorityV1,
        CollectionCandidateSearchInputV1,
        ContentId<CollectionCandidateSearchInputArtifact>,
    ) {
        let source = CollectionCandidateSearchAuthorityInput::new(
            task_id,
            id(format!("{label}-recovery")),
            id(format!("{label}-intent")),
            id(format!("{label}-oracle-outcome")),
            id(format!("{label}-oracle-claim")),
            SirCallerClaimId::new(format!("{label}-selection")).expect("claim"),
            CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
            CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        );
        let prepared = prepare_collection_candidate_search_input(&source).expect("search input");
        (
            CandidateWorkflowAuthorityV1::from_search_input(prepared.id(), prepared.input())
                .expect("workflow authority"),
            prepared.input().clone(),
            prepared.id(),
        )
    }

    fn request_from_state(state: &CandidateWorkflowStateV1) -> CandidateEpisodeRequestV1 {
        let CandidateWorkflowStateV1::CandidateEpisodeRequested { request, .. } = state else {
            panic!("episode request state");
        };
        request.clone()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the recorded consumer test keeps the complete transition trace explicit"
    )]
    fn run_recorded_chain(
        events: &mut SqliteEventStore,
        label: &str,
        reconcile_initial: bool,
    ) -> (
        MigrationWorkflowV1,
        CommandId,
        CollectionCandidateRevisionV1,
    ) {
        let task_id = TaskId::new();
        let workflow = MigrationWorkflowV1::new(task_id).expect("workflow");
        let (authority, search_input, search_input_id) = authority(task_id, label);
        let revision = revision(search_input_id, label);
        let revision_id = revision.identity().expect("revision id");
        let image = DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image");
        let profile = CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice;
        let limit = CandidateRevisionRoundLimit::new(2).expect("round limit");
        let open_command = CommandId::new();
        let open_time = ObservedAtUnixMillis::new(10);
        open_candidate_workflow(
            events,
            &workflow,
            authority.clone(),
            &search_input,
            &revision,
            revision_id,
            image.clone(),
            profile,
            limit,
            &open_command,
            open_time,
        )
        .expect("open workflow");

        let initial = dispatch(CandidateNativePublicationV1::Revision(revision_id));
        let initial_command = CommandId::new();
        request_candidate_native_build(
            events,
            &workflow,
            initial.clone(),
            &initial_command,
            ObservedAtUnixMillis::new(11),
        )
        .expect("initial build request");
        if reconcile_initial {
            require_candidate_native_build_reconciliation(
                events,
                &workflow,
                initial.clone(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(12),
            )
            .expect("reconciliation state");
        }
        let (failed, failed_id) = receipt(&initial, ExecutionOutcome::SubjectFailed, label);
        let followup_diagnostic = id::<CollectionCandidateNativeBuildDiagnosticArtifact>(format!(
            "{label}-native-followup-diagnostic"
        ));
        let failed_state = record_candidate_native_subject_failure(
            events,
            &workflow,
            failed_id,
            &failed,
            CandidateNativeDiagnosticV1::NativeFollowup(followup_diagnostic),
            &CommandId::new(),
            ObservedAtUnixMillis::new(13),
        )
        .expect("initial subject failure");
        assert!(matches!(
            failed_state.next_action().expect("failed next action"),
            CandidateWorkflowNextActionV1::PrepareCandidateEpisode {
                kind: CandidateEpisodeKindV1::NativeFollowup,
                ..
            }
        ));
        let state = request_candidate_episode(
            events,
            &workflow,
            EpisodeId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(14),
        )
        .expect("follow-up request");
        let request = request_from_state(&state);
        let followup = recorded_followup(&request, label);
        let followup_id = followup.identity().expect("follow-up id");
        record_candidate_native_followup(
            events,
            &workflow,
            &followup,
            followup_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(15),
        )
        .expect("recorded follow-up");

        let followup_build = dispatch(CandidateNativePublicationV1::NativeFollowup(followup_id));
        request_candidate_native_build(
            events,
            &workflow,
            followup_build.clone(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(16),
        )
        .expect("follow-up build request");
        let (failed, failed_id) = receipt(&followup_build, ExecutionOutcome::SubjectFailed, label);
        let repair_diagnostic = id::<CollectionCandidateNativeRepairBuildDiagnosticArtifact>(
            format!("{label}-native-repair-diagnostic"),
        );
        record_candidate_native_subject_failure(
            events,
            &workflow,
            failed_id,
            &failed,
            CandidateNativeDiagnosticV1::NativeRepair(repair_diagnostic),
            &CommandId::new(),
            ObservedAtUnixMillis::new(17),
        )
        .expect("follow-up subject failure");
        let state = request_candidate_episode(
            events,
            &workflow,
            EpisodeId::new(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(18),
        )
        .expect("repair request");
        let request = request_from_state(&state);
        let repair = recorded_repair(&request, followup_id, label);
        let repair_id = repair.identity().expect("repair id");
        record_candidate_native_repair(
            events,
            &workflow,
            &repair,
            repair_id,
            &CommandId::new(),
            ObservedAtUnixMillis::new(19),
        )
        .expect("recorded repair");

        let repair_build = dispatch(CandidateNativePublicationV1::NativeRepair(repair_id));
        request_candidate_native_build(
            events,
            &workflow,
            repair_build.clone(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(20),
        )
        .expect("repair build request");
        let (succeeded, succeeded_id) = receipt(&repair_build, ExecutionOutcome::Succeeded, label);
        let state = record_candidate_native_terminal(
            events,
            &workflow,
            succeeded_id,
            &succeeded,
            &CommandId::new(),
            ObservedAtUnixMillis::new(21),
        )
        .expect("native success");
        assert_eq!(
            state,
            CandidateWorkflowStateV1::Terminal(
                CandidateWorkflowTerminalV1::NativeCompilationSucceeded {
                    receipt: succeeded_id,
                }
            )
        );

        let replayed = open_candidate_workflow(
            events,
            &workflow,
            authority,
            &search_input,
            &revision,
            revision_id,
            image,
            profile,
            limit,
            &open_command,
            open_time,
        )
        .expect("old command exact replay returns current state");
        assert_eq!(replayed, state);

        let mut changed = initial;
        changed.job_id = JobId::new();
        assert!(matches!(
            request_candidate_native_build(
                events,
                &workflow,
                changed,
                &initial_command,
                ObservedAtUnixMillis::new(11),
            ),
            Err(CandidateWorkflowError::CommandConflict)
        ));
        (workflow, open_command, revision)
    }

    #[test]
    fn recorded_consumer_drives_two_materials_through_one_restart_safe_workflow() {
        let temporary = TempDir::new().expect("temporary directory");
        let database = temporary.path().join("events.sqlite3");
        let mut events = SqliteEventStore::open(&database).expect("event store");
        let (first, _, _) = run_recorded_chain(&mut events, "matrix-layout", true);
        drop(events);

        let mut events = SqliteEventStore::open(&database).expect("reopened event store");
        assert!(matches!(
            recover_candidate_workflow(&events, &first).expect("restart recovery"),
            CandidateWorkflowStateV1::Terminal(
                CandidateWorkflowTerminalV1::NativeCompilationSucceeded { .. }
            )
        ));
        run_recorded_chain(&mut events, "stream-window", false);
    }

    #[test]
    fn restart_recovers_unconsumed_episode_request_exactly() {
        let temporary = TempDir::new().expect("temporary directory");
        let database = temporary.path().join("events.sqlite3");
        let mut events = SqliteEventStore::open(&database).expect("event store");
        let task_id = TaskId::new();
        let workflow = MigrationWorkflowV1::new(task_id).expect("workflow");
        let (authority, search_input, search_input_id) = authority(task_id, "restart-episode");
        let revision = revision(search_input_id, "restart-episode");
        let revision_id = revision.identity().expect("revision id");
        open_candidate_workflow(
            &mut events,
            &workflow,
            authority,
            &search_input,
            &revision,
            revision_id,
            DockerImageId::new(format!("sha256:{}", "c".repeat(64))).expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            CandidateRevisionRoundLimit::new(1).expect("limit"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(1),
        )
        .expect("open");
        assert!(matches!(
            request_candidate_episode(
                &mut events,
                &workflow,
                EpisodeId::new(),
                &CommandId::new(),
                ObservedAtUnixMillis::new(2),
            ),
            Err(CandidateWorkflowError::InvalidTransition)
        ));
        let build = dispatch(CandidateNativePublicationV1::Revision(revision_id));
        request_candidate_native_build(
            &mut events,
            &workflow,
            build.clone(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("build request");
        let (failed, failed_id) =
            receipt(&build, ExecutionOutcome::SubjectFailed, "restart-episode");
        record_candidate_native_subject_failure(
            &mut events,
            &workflow,
            failed_id,
            &failed,
            CandidateNativeDiagnosticV1::NativeFollowup(id(b"restart diagnostic")),
            &CommandId::new(),
            ObservedAtUnixMillis::new(3),
        )
        .expect("subject failure");
        let episode_id = EpisodeId::new();
        let episode_command = CommandId::new();
        let expected = request_candidate_episode(
            &mut events,
            &workflow,
            episode_id,
            &episode_command,
            ObservedAtUnixMillis::new(4),
        )
        .expect("episode request");
        drop(events);

        let mut events = SqliteEventStore::open(&database).expect("reopened event store");
        assert_eq!(
            recover_candidate_workflow(&events, &workflow).expect("recover episode"),
            expected
        );
        assert_eq!(
            request_candidate_episode(
                &mut events,
                &workflow,
                episode_id,
                &episode_command,
                ObservedAtUnixMillis::new(4),
            )
            .expect("episode exact replay"),
            expected
        );
    }

    #[test]
    fn workflow_rejects_diagnostic_domain_drift() {
        let temporary = TempDir::new().expect("temporary directory");
        let mut events =
            SqliteEventStore::open(temporary.path().join("events.sqlite3")).expect("event store");
        let task_id = TaskId::new();
        let workflow = MigrationWorkflowV1::new(task_id).expect("workflow");
        let (authority, search_input, search_input_id) = authority(task_id, "domain-drift");
        let revision = revision(search_input_id, "domain-drift");
        let revision_id = revision.identity().expect("revision id");
        open_candidate_workflow(
            &mut events,
            &workflow,
            authority,
            &search_input,
            &revision,
            revision_id,
            DockerImageId::new(format!("sha256:{}", "b".repeat(64))).expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            CandidateRevisionRoundLimit::new(1).expect("limit"),
            &CommandId::new(),
            ObservedAtUnixMillis::new(1),
        )
        .expect("open");
        let build = dispatch(CandidateNativePublicationV1::Revision(revision_id));
        request_candidate_native_build(
            &mut events,
            &workflow,
            build.clone(),
            &CommandId::new(),
            ObservedAtUnixMillis::new(2),
        )
        .expect("build request");
        let (failed, failed_id) = receipt(&build, ExecutionOutcome::SubjectFailed, "domain-drift");
        assert!(matches!(
            record_candidate_native_subject_failure(
                &mut events,
                &workflow,
                failed_id,
                &failed,
                CandidateNativeDiagnosticV1::NativeRepair(id(b"wrong diagnostic domain")),
                &CommandId::new(),
                ObservedAtUnixMillis::new(3),
            ),
            Err(CandidateWorkflowError::BindingMismatch)
        ));
    }
}
