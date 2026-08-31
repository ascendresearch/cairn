//! In-process execution of one proposal step in the Controller workflow.
#![allow(clippy::missing_errors_doc)]

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use cairn_agent::{
    AdapterVersion, EpisodeBudget, HttpModelTransport, ModelOutputTokenLimit, ModelSelection,
    NativeProtocolCodec, ResolvedRuntimeModel,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, ProposalStepOutcomeV1, ProposalStepRequestV1,
    ProposalStepRoleRequestV1, ProposalStepRuntimeV1, SirTaskLimits,
    WorkflowToolExecutedObservationV1, WorkflowToolRequestV1, WorkflowToolWorker,
    execute_workflow_tools, run_proposal_step_episode,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Serialize};

use crate::ServerError;

/// Current-V1 model and budget policy for proposal steps in the main workflow.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalStepConfigV1 {
    pub resolved_runtime_model: PathBuf,
    pub selection: ModelSelection,
    pub budget: EpisodeBudget,
    pub max_output_tokens: ModelOutputTokenLimit,
    pub task_limits: SirTaskLimits,
}

impl ProposalStepConfigV1 {
    pub fn validate(&self) -> Result<(), ServerError> {
        let _ = self.resolved_model()?;
        Ok(())
    }

    pub fn resolve_paths(&mut self, base: &Path) {
        resolve(&mut self.resolved_runtime_model, base);
    }

    fn resolved_model(&self) -> Result<(ResolvedRuntimeModel, Vec<u8>), ServerError> {
        let bytes = fs::read(&self.resolved_runtime_model)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        let model: ResolvedRuntimeModel = cairn_codec::from_slice(&bytes)
            .map_err(|error| ServerError::Configuration(error.to_string()))?;
        if model.canonical_bytes().map_err(configuration_error)? != bytes
            || model.provider() != &self.selection.provider
            || model.wire_model() != &self.selection.model
            || model.deployment() != &self.selection.deployment
            || self.selection.adapter_version
                != AdapterVersion::new("native-protocol-v1").map_err(configuration_error)?
            || self.max_output_tokens > model.capabilities().max_output_tokens()
        {
            return Err(ServerError::Configuration(
                "resolved runtime model changed the configured proposal-step policy".into(),
            ));
        }
        Ok((model, bytes))
    }

    pub fn runtime(&self, episode_id: EpisodeId) -> Result<ProposalStepRuntimeV1, ServerError> {
        let (_, bytes) = self.resolved_model()?;
        Ok(ProposalStepRuntimeV1::new(
            episode_id,
            ContentId::<AgentResolvedRuntimeModelArtifact>::derive(&bytes)
                .map_err(runtime_error)?,
            self.selection.clone(),
            self.budget.clone(),
            self.max_output_tokens,
            self.task_limits,
        ))
    }
}

/// Runs a proposal step directly against the Controller-owned durable stores.
pub(crate) fn run_proposal_step<E, C>(
    config: &ProposalStepConfigV1,
    events: &mut E,
    content: &mut C,
    request: &ProposalStepRequestV1,
) -> Result<ProposalStepOutcomeV1, ServerError>
where
    E: EventStore,
    C: ContentStore,
{
    let (model, model_bytes) = config.resolved_model()?;
    let expected_model = request.runtime().model_configuration();
    let archived_model = content
        .put::<AgentResolvedRuntimeModelArtifact>(&mut Cursor::new(&model_bytes))
        .map_err(runtime_error)?
        .content_id;
    if archived_model != expected_model {
        return Err(ServerError::MigrationWorkflow(
            "proposal step changed its resolved runtime-model identity".into(),
        ));
    }
    let request_id = request.identity().map_err(runtime_error)?;
    let (task_id, role) = proposal_step_scope(request);
    tracing::info!(
        target: "cairn.server.controller-workflow",
        event = "proposal_step_started",
        task_id = %task_id,
        episode_id = %request.runtime().episode_id(),
        request_id = %request_id,
        role,
        "Controller workflow proposal step started"
    );
    let codec = NativeProtocolCodec::from_config(model.protocol()).map_err(runtime_error)?;
    let credential_base = std::env::current_dir().map_err(runtime_error)?;
    let mut transport = HttpModelTransport::new(&model, credential_base).map_err(runtime_error)?;
    let outcome =
        run_proposal_step_episode(events, content, &mut transport, codec, request.clone())
            .map_err(runtime_error)?;
    outcome.validate_against(request).map_err(runtime_error)?;
    let outcome_class = match &outcome {
        ProposalStepOutcomeV1::Terminal { .. } => "proposal-recorded",
        ProposalStepOutcomeV1::WorkerRequest { .. } => "worker-requested",
    };
    tracing::info!(
        target: "cairn.server.controller-workflow",
        event = "proposal_step_completed",
        task_id = %task_id,
        episode_id = %request.runtime().episode_id(),
        request_id = %request_id,
        role,
        outcome_class,
        "Controller workflow proposal step completed"
    );
    Ok(outcome)
}

/// Executes Controller-authorized external tool requests against the same workflow stores.
pub fn execute_controller_workflow_tools<E, C, W>(
    events: &mut E,
    content: &mut C,
    request: &ProposalStepRequestV1,
    worker_request: &WorkflowToolRequestV1,
    worker: &mut W,
) -> Result<Vec<WorkflowToolExecutedObservationV1>, ServerError>
where
    E: EventStore,
    C: ContentStore,
    W: WorkflowToolWorker,
{
    execute_workflow_tools(events, content, request, worker_request, worker).map_err(runtime_error)
}

fn resolve(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
}

fn proposal_step_scope(request: &ProposalStepRequestV1) -> (TaskId, &'static str) {
    match request.role() {
        ProposalStepRoleRequestV1::Sir { task_id, .. } => (*task_id, "sir"),
        ProposalStepRoleRequestV1::OracleStrategy { workspace, .. } => {
            (workspace.task_id(), "oracle-strategy")
        }
        ProposalStepRoleRequestV1::CandidateStrategy { workspace, .. } => {
            (workspace.task_id(), "candidate-strategy")
        }
    }
}

fn configuration_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::Configuration(error.to_string())
}

fn runtime_error(error: impl std::fmt::Display) -> ServerError {
    ServerError::MigrationWorkflow(error.to_string())
}
