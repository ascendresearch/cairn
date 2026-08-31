//! Local product API over the same durable Controller aggregate used by normal execution.

use std::{
    collections::BTreeMap, os::unix::fs::FileTypeExt, path::Path, str::FromStr, time::Duration,
};

use cairn_admission::{
    UserIntentAuthorityGrantV1, UserIntentDecisionArtifact, UserIntentDecisionResponseV1,
    UserIntentDecisionV1,
};
use cairn_migration::{
    IntentDecisionRequestBatchArtifact, IntentDecisionRequestBatchV1,
    OracleAdmissionOutcomeArtifact, OracleAdmissionOutcomeV1, OracleClaimAdmissionStatusV1,
    OracleControlDispatchV1, OracleControlRunV1, OracleControlWorker, OracleControlWorkerBindingV1,
    OracleControlWorkerError, ProposalStepRequestV1, ProposalStepRoleRequestV1,
    ProposalStepTaskSnapshotV1, SirIntentHypothesisSetProposalArtifact, SirTaskWorkspace,
    TrustedOracleControlObservationV1, UserIntentDecisionRequestArtifact,
    UserIntentDecisionRequestV1,
};
use cairn_protocol::{
    AggregateKind, ContentId, ContentType, EpisodeId, EventSequence, ObservedAtUnixMillis, TaskId,
};
use cairn_record::{ContentStore, EventStore};
use cairn_sdk::{
    AppApiErrorCodeV1, CairnRequestV1, CairnResponseV1, IntentReviewRequestResourceV1,
    IntentReviewResourceV1, TaskAttentionV1, TaskPhaseV1, TaskProgressItemV1, TaskProgressPageV1,
    TaskResourceV1, read_frame, write_frame,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::{UnixListener, UnixStream};

use crate::{
    AppApiConfigV1, ControllerWorkflowStateV1, ControllerWorkflowV1, ServerConfig, ServerError,
    cancel_controller_workflow, drive_controller_workflow_once, freeze_sir_controller_request,
    initialize_product_oracle_exploration, observed_now, reauthorize_controller_intent_admission,
    record_controller_user_intent_decision, recover_controller_workflow,
};

const CONTROLLER_WORKFLOW_KIND: &str = "controller-workflow";

pub(crate) async fn run_listener(
    server: ServerConfig,
    config: AppApiConfigV1,
) -> Result<(), ServerError> {
    prepare_socket(&config.unix_socket)?;
    let listener = UnixListener::bind(&config.unix_socket)
        .map_err(|error| ServerError::Startup(error.to_string()))?;
    tracing::info!(
        target: "cairn.server.app-api",
        event = "app_api_listener_ready",
        unix_socket = %config.unix_socket.display(),
        "local App API listener ready"
    );
    let supervisor_server = server.clone();
    let supervisor_config = config.clone();
    tokio::spawn(async move {
        supervise_tasks(supervisor_server, supervisor_config).await;
    });
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|error| ServerError::Startup(error.to_string()))?;
        let request_server = server.clone();
        let request_config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, &request_server, &request_config).await {
                tracing::warn!(
                    target: "cairn.server.app-api",
                    event = "app_api_connection_failed",
                    error_class = %error,
                    "App API connection failed"
                );
            }
        });
    }
}

async fn supervise_tasks(server: ServerConfig, config: AppApiConfigV1) {
    let mut attempted = BTreeMap::<TaskId, EventSequence>::new();
    let mut unavailable_controls = UnavailableOracleControls;
    loop {
        match list_tasks(&server) {
            Ok(tasks) => {
                for task in tasks {
                    if attempted.get(&task.task_id()) == Some(&task.latest_sequence()) {
                        continue;
                    }
                    attempted.insert(task.task_id(), task.latest_sequence());
                    if task.phase() == TaskPhaseV1::PreparingOracle
                        && task.attention() == Some(TaskAttentionV1::OracleWorkspace)
                    {
                        let workflow = match ControllerWorkflowV1::new(task.task_id()) {
                            Ok(workflow) => workflow,
                            Err(error) => {
                                tracing::error!(
                                    target: "cairn.server.app-api",
                                    event = "oracle_workspace_identity_failed",
                                    task_id = %task.task_id(),
                                    error_class = %error,
                                    "product Oracle workspace rejected task identity"
                                );
                                continue;
                            }
                        };
                        if let Err(error) = initialize_product_oracle_exploration(
                            &server,
                            &config.proposal_step,
                            &workflow,
                            config.oracle.coverage_profile,
                            config.oracle.adversarial_policy,
                            config.oracle.budget,
                            &config.oracle.documentation,
                            &config.oracle.build_and_tests,
                        ) {
                            tracing::warn!(
                                target: "cairn.server.app-api",
                                event = "oracle_workspace_initialization_failed",
                                task_id = %task.task_id(),
                                sequence = task.latest_sequence().get(),
                                error_class = %error,
                                "product Oracle workspace initialization failed"
                            );
                        }
                        continue;
                    }
                    if (task.attention().is_some()
                        && task.phase() != TaskPhaseV1::AwaitingIntentReview)
                        || terminal_phase(task.phase())
                    {
                        continue;
                    }
                    let workflow = match ControllerWorkflowV1::new(task.task_id()) {
                        Ok(workflow) => workflow,
                        Err(error) => {
                            tracing::error!(
                                target: "cairn.server.app-api",
                                event = "task_supervisor_identity_failed",
                                task_id = %task.task_id(),
                                error_class = %error,
                                "task supervisor rejected aggregate identity"
                            );
                            continue;
                        }
                    };
                    if let Err(error) = drive_controller_workflow_once(
                        &server,
                        &config.proposal_step,
                        &config.intent_admission,
                        &workflow,
                        &mut unavailable_controls,
                    )
                    .await
                    {
                        tracing::warn!(
                            target: "cairn.server.app-api",
                            event = "task_supervisor_step_failed",
                            task_id = %task.task_id(),
                            sequence = task.latest_sequence().get(),
                            error_class = %error,
                            "task supervisor step failed"
                        );
                    }
                }
            }
            Err(error) => tracing::error!(
                target: "cairn.server.app-api",
                event = "task_supervisor_discovery_failed",
                error_class = %error,
                "task supervisor could not discover Controller aggregates"
            ),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

struct UnavailableOracleControls;

impl OracleControlWorker for UnavailableOracleControls {
    fn prepare(
        &mut self,
        _run: &OracleControlRunV1,
    ) -> Result<OracleControlWorkerBindingV1, OracleControlWorkerError> {
        Err(OracleControlWorkerError::NotStarted(
            "no qualified Oracle mechanism runner is configured".into(),
        ))
    }

    fn execute(
        &mut self,
        _dispatch: &OracleControlDispatchV1,
    ) -> Result<TrustedOracleControlObservationV1, OracleControlWorkerError> {
        Err(OracleControlWorkerError::NotStarted(
            "no qualified Oracle mechanism runner is configured".into(),
        ))
    }
}

fn terminal_phase(phase: TaskPhaseV1) -> bool {
    matches!(
        phase,
        TaskPhaseV1::OracleAccepted
            | TaskPhaseV1::OraclePartial
            | TaskPhaseV1::OracleRejected
            | TaskPhaseV1::Cancelled
            | TaskPhaseV1::Blocked
    )
}

async fn handle_connection(
    mut stream: UnixStream,
    server: &ServerConfig,
    config: &AppApiConfigV1,
) -> Result<(), ServerError> {
    let request: CairnRequestV1 = read_frame(&mut stream)
        .await
        .map_err(|error| ServerError::MigrationWorkflow(error.to_string()))?;
    let response = match handle_request(server, config, request) {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                target: "cairn.server.app-api",
                event = "app_api_request_rejected",
                error_class = %error,
                "App API request rejected"
            );
            CairnResponseV1::Error {
                code: classify_error(&error),
            }
        }
    };
    write_frame(&mut stream, &response)
        .await
        .map_err(|error| ServerError::MigrationWorkflow(error.to_string()))
}

#[allow(
    clippy::too_many_lines,
    reason = "the current V1 App API keeps its closed request variants in one auditable dispatch table"
)]
fn handle_request(
    server: &ServerConfig,
    config: &AppApiConfigV1,
    request: CairnRequestV1,
) -> Result<CairnResponseV1, ServerError> {
    match request {
        CairnRequestV1::SubmitTask {
            command_id,
            task_id,
            submission,
        } => {
            let workspace = SirTaskWorkspace::from_sources(
                submission
                    .sources()
                    .iter()
                    .map(|source| (source.path().clone(), source.text().to_owned()))
                    .collect(),
                config.proposal_step.task_limits,
            )
            .map_err(migration_error)?;
            let request = ProposalStepRequestV1::new(
                config.proposal_step.runtime(EpisodeId::new())?,
                ProposalStepRoleRequestV1::Sir {
                    task_id,
                    recovery_request: submission.recovery_request().clone(),
                    task: ProposalStepTaskSnapshotV1::from_workspace(&workspace),
                },
            )
            .map_err(migration_error)?;
            let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
            freeze_sir_controller_request(
                server,
                &workflow,
                &request,
                &command_id,
                command_observed_at(server, task_id, command_id)?,
            )?;
            let task = task_resource(server, task_id)?;
            tracing::info!(
                target: "cairn.server.app-api",
                event = "task_submitted",
                task_id = %task_id,
                command_id = %command_id,
                source_count = submission.sources().len(),
                "task accepted at the product boundary"
            );
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
        CairnRequestV1::ListTasks => Ok(CairnResponseV1::Tasks {
            tasks: list_tasks(server)?,
        }),
        CairnRequestV1::GetTask { task_id } => Ok(CairnResponseV1::Task {
            task: task_resource(server, task_id)?,
        }),
        CairnRequestV1::GetTaskProgress {
            task_id,
            after_sequence,
        } => Ok(CairnResponseV1::Progress {
            page: progress(server, task_id, after_sequence)?,
        }),
        CairnRequestV1::CancelTask {
            command_id,
            task_id,
        } => {
            let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
            let mut events = open_events(server)?;
            cancel_controller_workflow(
                &mut events,
                &workflow,
                &command_id,
                command_observed_at(server, task_id, command_id)?,
            )
            .map_err(migration_error)?;
            let task = task_resource(server, task_id)?;
            tracing::info!(
                target: "cairn.server.app-api",
                event = "task_cancelled",
                task_id = %task_id,
                command_id = %command_id,
                "task cancellation committed"
            );
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
        CairnRequestV1::GetIntentReview { task_id } => Ok(CairnResponseV1::IntentReview {
            review: Box::new(intent_review(server, task_id)?),
        }),
        CairnRequestV1::SelectIntentHypothesis {
            command_id,
            task_id,
            request_id,
            hypothesis,
            authority_scope,
        } => {
            let content = open_content(server)?;
            let request: UserIntentDecisionRequestV1 =
                load_canonical::<UserIntentDecisionRequestArtifact, _>(&content, request_id)?;
            let option = request
                .options()
                .iter()
                .find(|option| option.hypothesis() == &hypothesis)
                .ok_or_else(|| {
                    ServerError::MigrationWorkflow(
                        "selected hypothesis is not an option for the exact request".into(),
                    )
                })?;
            let grant = UserIntentAuthorityGrantV1::new(
                task_id,
                config.intent_authority_subject.clone(),
                authority_scope.clone(),
            );
            let decision = UserIntentDecisionV1::new(
                request_id,
                grant.identity().map_err(migration_error)?,
                UserIntentDecisionResponseV1::SelectHypothesis { hypothesis },
            );
            let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
            record_controller_user_intent_decision(
                server,
                &workflow,
                &grant,
                &decision,
                &command_id,
                command_observed_at(server, task_id, command_id)?,
            )?;
            let task = task_resource(server, task_id)?;
            tracing::info!(
                target: "cairn.server.app-api",
                event = "intent_decision_recorded",
                task_id = %task_id,
                command_id = %command_id,
                request_id = %request_id,
                hypothesis_id = option.hypothesis().as_str(),
                authority_claim_count = authority_scope.claims().len(),
                "operator intent decision committed"
            );
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
        CairnRequestV1::KeepIntentUnknown {
            command_id,
            task_id,
            request_id,
            authority_scope,
        } => {
            let grant = UserIntentAuthorityGrantV1::new(
                task_id,
                config.intent_authority_subject.clone(),
                authority_scope.clone(),
            );
            let decision = UserIntentDecisionV1::new(
                request_id,
                grant.identity().map_err(migration_error)?,
                UserIntentDecisionResponseV1::KeepUnknown,
            );
            record_app_intent_decision(
                server,
                task_id,
                command_id,
                request_id,
                &authority_scope,
                &grant,
                &decision,
                "keep-unknown",
            )
        }
        CairnRequestV1::ProvideIntentClaim {
            command_id,
            task_id,
            request_id,
            authority_scope,
            claim,
        } => {
            let grant = UserIntentAuthorityGrantV1::new(
                task_id,
                config.intent_authority_subject.clone(),
                authority_scope.clone(),
            );
            let decision = UserIntentDecisionV1::new(
                request_id,
                grant.identity().map_err(migration_error)?,
                UserIntentDecisionResponseV1::ProvideAuthoritativeClaim { claim },
            );
            record_app_intent_decision(
                server,
                task_id,
                command_id,
                request_id,
                &authority_scope,
                &grant,
                &decision,
                "provide-authoritative-claim",
            )
        }
        CairnRequestV1::ReconcileIntentAdmission {
            command_id,
            task_id,
        } => {
            let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
            reauthorize_controller_intent_admission(
                server,
                &config.intent_admission,
                &workflow,
                &command_id,
                command_observed_at(server, task_id, command_id)?,
            )?;
            let task = task_resource(server, task_id)?;
            tracing::info!(
                target: "cairn.server.app-api",
                event = "intent_admission_reauthorized",
                task_id = %task_id,
                command_id = %command_id,
                "blocked Intent Admission reauthorized against current configuration"
            );
            Ok(CairnResponseV1::Mutation { command_id, task })
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "exact task, command, request, scope, grant, and decision authorities stay explicit"
)]
fn record_app_intent_decision(
    server: &ServerConfig,
    task_id: TaskId,
    command_id: cairn_protocol::CommandId,
    request_id: ContentId<UserIntentDecisionRequestArtifact>,
    authority_scope: &cairn_admission::UserIntentAuthorityScopeV1,
    grant: &UserIntentAuthorityGrantV1,
    decision: &UserIntentDecisionV1,
    response_kind: &'static str,
) -> Result<CairnResponseV1, ServerError> {
    let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
    record_controller_user_intent_decision(
        server,
        &workflow,
        grant,
        decision,
        &command_id,
        command_observed_at(server, task_id, command_id)?,
    )?;
    let task = task_resource(server, task_id)?;
    tracing::info!(
        target: "cairn.server.app-api",
        event = "intent_decision_recorded",
        task_id = %task_id,
        command_id = %command_id,
        request_id = %request_id,
        response_kind,
        authority_claim_count = authority_scope.claims().len(),
        "operator intent decision committed"
    );
    Ok(CairnResponseV1::Mutation { command_id, task })
}

fn list_tasks(server: &ServerConfig) -> Result<Vec<TaskResourceV1>, ServerError> {
    let events = open_events(server)?;
    let kind = AggregateKind::new(CONTROLLER_WORKFLOW_KIND).map_err(migration_error)?;
    events
        .list_streams(&kind)
        .map_err(migration_error)?
        .into_iter()
        .map(|stream| {
            let task_id = TaskId::from_str(stream.id.as_str()).map_err(migration_error)?;
            task_resource(server, task_id)
        })
        .collect()
}

fn task_resource(server: &ServerConfig, task_id: TaskId) -> Result<TaskResourceV1, ServerError> {
    let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
    let events = open_events(server)?;
    let state = recover_controller_workflow(&events, &workflow).map_err(migration_error)?;
    if state == ControllerWorkflowStateV1::NotFound {
        return Err(ServerError::RegistryEntryNotFound("task".into()));
    }
    let history = events
        .read_stream(&workflow_stream(task_id)?, None)
        .map_err(migration_error)?;
    let latest = history
        .last()
        .ok_or_else(|| ServerError::MigrationWorkflow("task stream is empty".into()))?
        .sequence;
    let (phase, attention) = phase_for_state(server, &state)?;
    Ok(TaskResourceV1::new(task_id, latest, phase, attention))
}

fn progress(
    server: &ServerConfig,
    task_id: TaskId,
    after_sequence: Option<EventSequence>,
) -> Result<TaskProgressPageV1, ServerError> {
    let task = task_resource(server, task_id)?;
    let events = open_events(server)?;
    let items = events
        .read_stream(&workflow_stream(task_id)?, after_sequence)
        .map_err(migration_error)?
        .into_iter()
        .map(|event| TaskProgressItemV1 {
            sequence: event.sequence,
            event_id: event.event_id,
            observed_at: ObservedAtUnixMillis::new(event.observed_at_unix_ms),
            phase: phase_for_schema(event.schema_name.as_str()),
        })
        .collect();
    Ok(TaskProgressPageV1 { task, items })
}

fn intent_review(
    server: &ServerConfig,
    task_id: TaskId,
) -> Result<IntentReviewResourceV1, ServerError> {
    let workflow = ControllerWorkflowV1::new(task_id).map_err(migration_error)?;
    let events = open_events(server)?;
    let state = recover_controller_workflow(&events, &workflow).map_err(migration_error)?;
    let (proposal, requests) = match state {
        ControllerWorkflowStateV1::SirProposed { proposal, .. } => (proposal, None),
        ControllerWorkflowStateV1::AwaitingUserIntentDecision {
            proposal, requests, ..
        } => (proposal, Some(requests)),
        _ => {
            return Err(ServerError::MigrationWorkflow(
                "task has no SIR proposal ready for intent review".into(),
            ));
        }
    };
    let content = open_content(server)?;
    Ok(IntentReviewResourceV1 {
        task_id,
        proposal_id: proposal,
        proposal: load_canonical::<SirIntentHypothesisSetProposalArtifact, _>(&content, proposal)?,
        requests_id: requests,
        requests: requests
            .map(|id| {
                load_canonical::<IntentDecisionRequestBatchArtifact, IntentDecisionRequestBatchV1>(
                    &content, id,
                )
            })
            .transpose()?
            .map(|batch| {
                batch
                    .requests()
                    .iter()
                    .cloned()
                    .map(|request| {
                        Ok(IntentReviewRequestResourceV1 {
                            request_id: request.identity().map_err(migration_error)?,
                            request,
                        })
                    })
                    .collect::<Result<Vec<_>, ServerError>>()
            })
            .transpose()?
            .unwrap_or_default(),
    })
}

fn phase_for_state(
    server: &ServerConfig,
    state: &ControllerWorkflowStateV1,
) -> Result<(TaskPhaseV1, Option<TaskAttentionV1>), ServerError> {
    Ok(match state {
        ControllerWorkflowStateV1::NotFound => {
            return Err(ServerError::RegistryEntryNotFound("task".into()));
        }
        ControllerWorkflowStateV1::Cancelled => (TaskPhaseV1::Cancelled, None),
        ControllerWorkflowStateV1::Frozen(_)
        | ControllerWorkflowStateV1::SirEpisodeAuthorized(_) => {
            (TaskPhaseV1::RecoveringIntent, None)
        }
        ControllerWorkflowStateV1::SirProposed { .. }
        | ControllerWorkflowStateV1::AwaitingUserIntentDecision { .. } => (
            TaskPhaseV1::AwaitingIntentReview,
            Some(TaskAttentionV1::IntentReview),
        ),
        ControllerWorkflowStateV1::UserIntentDecisionRecorded { decision, .. } => {
            let content = open_content(server)?;
            let body: UserIntentDecisionV1 =
                load_canonical::<UserIntentDecisionArtifact, _>(&content, *decision)?;
            if matches!(body.response(), UserIntentDecisionResponseV1::KeepUnknown) {
                (TaskPhaseV1::Blocked, None)
            } else {
                (TaskPhaseV1::AdmittingIntent, None)
            }
        }
        ControllerWorkflowStateV1::IntentAdmissionAuthorized { .. } => {
            (TaskPhaseV1::AdmittingIntent, None)
        }
        ControllerWorkflowStateV1::IntentAdmissionBlocked { .. } => (
            TaskPhaseV1::Blocked,
            Some(TaskAttentionV1::IntentAdmissionReconciliation),
        ),
        ControllerWorkflowStateV1::AdmittedIntent { .. } => (
            TaskPhaseV1::PreparingOracle,
            Some(TaskAttentionV1::OracleWorkspace),
        ),
        ControllerWorkflowStateV1::OracleExplorationOpened(_)
        | ControllerWorkflowStateV1::OracleStrategyAuthorized(_) => {
            (TaskPhaseV1::ExploringOracle, None)
        }
        ControllerWorkflowStateV1::OraclePortfolioFrozen(_) => (
            TaskPhaseV1::AwaitingOracleControls,
            Some(TaskAttentionV1::OracleMechanisms),
        ),
        ControllerWorkflowStateV1::OracleAdmissionAuthorized(_)
        | ControllerWorkflowStateV1::OracleControlAuthorized { .. }
        | ControllerWorkflowStateV1::OracleControlsObserved { .. } => {
            (TaskPhaseV1::RunningOracleControls, None)
        }
        ControllerWorkflowStateV1::OracleAdmitted { outcome, .. } => {
            let content = open_content(server)?;
            let body: OracleAdmissionOutcomeV1 =
                load_canonical::<OracleAdmissionOutcomeArtifact, _>(&content, *outcome)?;
            let phase = if body
                .claims()
                .iter()
                .any(|claim| claim.status() == OracleClaimAdmissionStatusV1::Rejected)
            {
                TaskPhaseV1::OracleRejected
            } else if body
                .claims()
                .iter()
                .any(|claim| claim.status() == OracleClaimAdmissionStatusV1::Partial)
            {
                TaskPhaseV1::OraclePartial
            } else {
                TaskPhaseV1::OracleAccepted
            };
            (phase, None)
        }
        ControllerWorkflowStateV1::CandidateOracleContractFrozen(_)
        | ControllerWorkflowStateV1::CandidateProposalRequestFrozen(_)
        | ControllerWorkflowStateV1::CandidateProposalEpisodeAuthorized(_)
        | ControllerWorkflowStateV1::CandidateProposed { .. }
        | ControllerWorkflowStateV1::CandidateBuildFrozen(_)
        | ControllerWorkflowStateV1::CandidateBuildAuthorized(_)
        | ControllerWorkflowStateV1::CandidateBuildObserved { .. }
        | ControllerWorkflowStateV1::CandidateAdmissionAuthorized(_)
        | ControllerWorkflowStateV1::Terminal { .. } => (TaskPhaseV1::Blocked, None),
    })
}

fn phase_for_schema(schema: &str) -> TaskPhaseV1 {
    match schema {
        "migration.controller-workflow-frozen" | "migration.controller-sir-episode-authorized" => {
            TaskPhaseV1::RecoveringIntent
        }
        "migration.controller-sir-proposal-recorded"
        | "migration.controller-intent-decision-requests-recorded" => {
            TaskPhaseV1::AwaitingIntentReview
        }
        "migration.controller-user-intent-decision-recorded"
        | "migration.controller-intent-admission-authorized"
        | "migration.controller-admitted-intent-recorded" => TaskPhaseV1::AdmittingIntent,
        "migration.controller-oracle-exploration-opened" => TaskPhaseV1::PreparingOracle,
        "migration.controller-oracle-strategy-authorized"
        | "migration.controller-oracle-strategy-observations-recorded"
        | "migration.controller-oracle-strategy-submission-recorded" => {
            TaskPhaseV1::ExploringOracle
        }
        "migration.controller-oracle-portfolio-frozen" => TaskPhaseV1::AwaitingOracleControls,
        "migration.controller-oracle-admission-authorized"
        | "migration.controller-oracle-control-authorized"
        | "migration.controller-oracle-control-observed" => TaskPhaseV1::RunningOracleControls,
        "migration.controller-oracle-admission-recorded" => TaskPhaseV1::OracleAccepted,
        "migration.controller-workflow-cancelled" => TaskPhaseV1::Cancelled,
        _ => TaskPhaseV1::Blocked,
    }
}

fn workflow_stream(task_id: TaskId) -> Result<cairn_record::StreamId, ServerError> {
    Ok(cairn_record::StreamId {
        kind: AggregateKind::new(CONTROLLER_WORKFLOW_KIND).map_err(migration_error)?,
        id: cairn_protocol::AggregateId::new(task_id.to_string()).map_err(migration_error)?,
    })
}

fn command_observed_at(
    server: &ServerConfig,
    task_id: TaskId,
    command_id: cairn_protocol::CommandId,
) -> Result<ObservedAtUnixMillis, ServerError> {
    let events = open_events(server)?;
    let prior = events
        .read_stream(&workflow_stream(task_id)?, None)
        .map_err(migration_error)?
        .into_iter()
        .find(|event| event.command_id == command_id);
    prior.map_or_else(observed_now, |event| {
        Ok(ObservedAtUnixMillis::new(event.observed_at_unix_ms))
    })
}

fn open_events(server: &ServerConfig) -> Result<SqliteEventStore, ServerError> {
    SqliteEventStore::open(&server.storage.event_database).map_err(migration_error)
}

fn open_content(server: &ServerConfig) -> Result<SqliteContentStore, ServerError> {
    SqliteContentStore::open(
        &server.storage.content_database,
        &server.storage.content_directory,
    )
    .map_err(migration_error)
}

fn load_canonical<T: ContentType, V: DeserializeOwned + Serialize>(
    content: &SqliteContentStore,
    id: ContentId<T>,
) -> Result<V, ServerError> {
    let mut bytes = Vec::new();
    content.write_to(&id, &mut bytes).map_err(migration_error)?;
    let value = cairn_codec::from_slice(&bytes).map_err(migration_error)?;
    if cairn_codec::to_vec(&value).map_err(migration_error)? != bytes
        || ContentId::<T>::derive(&bytes).map_err(migration_error)? != id
    {
        return Err(ServerError::MigrationWorkflow(
            "App API content identity verification failed".into(),
        ));
    }
    Ok(value)
}

fn prepare_socket(path: &Path) -> Result<(), ServerError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ServerError::Startup(error.to_string()))?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(path).map_err(|error| ServerError::Startup(error.to_string()))
        }
        Ok(_) => Err(ServerError::Startup(
            "App API path exists and is not a Unix socket".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ServerError::Startup(error.to_string())),
    }
}

fn classify_error(error: &ServerError) -> AppApiErrorCodeV1 {
    match error {
        ServerError::RegistryEntryNotFound(_) => AppApiErrorCodeV1::TaskNotFound,
        ServerError::MigrationWorkflow(message) if message.contains("not awaiting") => {
            AppApiErrorCodeV1::NotReady
        }
        ServerError::MigrationWorkflow(message) if message.contains("command") => {
            AppApiErrorCodeV1::Conflict
        }
        ServerError::Configuration(_) => AppApiErrorCodeV1::InvalidRequest,
        _ => AppApiErrorCodeV1::Internal,
    }
}

fn migration_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
