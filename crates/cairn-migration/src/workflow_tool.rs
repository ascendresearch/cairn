//! Controller-owned bridge from a proposal-step Worker request to a managed Worker observation.

use cairn_agent::{
    AgentEpisode, AgentEpisodeState, AgentStepState, CanonicalToolResult, OperationResult,
    PreparedToolOperation, ToolGateway, ToolGatewayError, ToolOperationState,
    authorize_tool_operation, begin_tool_operation, execute_tool_operation, recover_agent_episode,
    recover_tool_operation,
};
use cairn_execution::{ExecutionReceipt, ExecutionReceiptArtifact, JobContractArtifact};
use cairn_protocol::{
    AttemptId, CommandId, ContentId, ContentType, JobId, ObservedAtUnixMillis, OperationId,
    SchemaVersion,
};
use cairn_record::{ContentStore, EventStore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    OracleExplorationObservationV1, OracleObservationPayloadV1, ProposalStepRequestV1,
    ProposalStepRoleRequestV1, WorkflowToolControllerObservationArtifact, WorkflowToolOperationV1,
    WorkflowToolRequestArtifact, WorkflowToolRequestV1,
};

/// Identity of one exact Controller-authorized Worker dispatch.
pub enum WorkflowToolDispatchArtifact {}

impl ContentType for WorkflowToolDispatchArtifact {
    const DOMAIN: &'static str = "migration.workflow-tool-dispatch.v1";
}

/// Controller-recomputed effect observation returned to the same Agent episode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkflowToolControllerObservationV1 {
    schema_version: SchemaVersion,
    dispatch: WorkflowToolDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowToolControllerObservationWire {
    schema_version: SchemaVersion,
    dispatch: WorkflowToolDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

impl WorkflowToolControllerObservationV1 {
    fn from_worker(
        dispatch: WorkflowToolDispatchV1,
        observed: WorkflowToolWorkerObservationV1,
    ) -> Result<Self, WorkflowToolError> {
        let value = Self {
            schema_version: schema_v1(),
            dispatch,
            receipt_id: observed.receipt_id,
            receipt: observed.receipt,
            observation: observed.observation,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn dispatch(&self) -> &WorkflowToolDispatchV1 {
        &self.dispatch
    }

    #[must_use]
    pub const fn observation(&self) -> &serde_json::Value {
        &self.observation
    }

    /// Derives the exact Controller observation identity.
    ///
    /// # Errors
    ///
    /// Rejects schema, dispatch, Worker receipt, or canonical codec drift.
    pub fn identity(
        &self,
    ) -> Result<ContentId<WorkflowToolControllerObservationArtifact>, WorkflowToolError> {
        self.validate()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec_error)?).map_err(codec_error)
    }

    fn validate(&self) -> Result<(), WorkflowToolError> {
        if self.schema_version != schema_v1()
            || self.receipt.job_id() != self.dispatch.worker.job_id
            || self.receipt.attempt_id() != self.dispatch.worker.attempt_id
            || self.receipt.contract_id() != self.dispatch.worker.contract_id
        {
            return invalid("Controller observation changed its authorized Worker binding");
        }
        let bytes = cairn_codec::to_vec(&self.receipt).map_err(codec_error)?;
        if ContentId::<ExecutionReceiptArtifact>::derive(&bytes).map_err(codec_error)?
            != self.receipt_id
        {
            return invalid("Controller observation changed its Worker receipt identity");
        }
        Ok(())
    }
}

impl TryFrom<WorkflowToolControllerObservationWire> for WorkflowToolControllerObservationV1 {
    type Error = WorkflowToolError;

    fn try_from(wire: WorkflowToolControllerObservationWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            dispatch: wire.dispatch,
            receipt_id: wire.receipt_id,
            receipt: wire.receipt,
            observation: wire.observation,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for WorkflowToolControllerObservationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        WorkflowToolControllerObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Exact Controller effect plus its optional Oracle-domain projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkflowToolExecutedObservationV1 {
    controller: WorkflowToolControllerObservationV1,
    oracle_payload: Option<OracleObservationPayloadV1>,
    oracle_observation: Option<OracleExplorationObservationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowToolExecutedObservationWire {
    controller: WorkflowToolControllerObservationV1,
    oracle_payload: Option<OracleObservationPayloadV1>,
    oracle_observation: Option<OracleExplorationObservationV1>,
}

impl WorkflowToolExecutedObservationV1 {
    #[must_use]
    pub const fn controller(&self) -> &WorkflowToolControllerObservationV1 {
        &self.controller
    }

    #[must_use]
    pub const fn oracle_payload(&self) -> Option<&OracleObservationPayloadV1> {
        self.oracle_payload.as_ref()
    }

    #[must_use]
    pub const fn oracle_observation(&self) -> Option<&OracleExplorationObservationV1> {
        self.oracle_observation.as_ref()
    }

    fn validate(&self) -> Result<(), WorkflowToolError> {
        self.controller.validate()?;
        match (&self.oracle_payload, &self.oracle_observation) {
            (None, None) => Ok(()),
            (Some(payload), Some(observation)) => {
                let controller_id = self.controller.identity()?;
                if payload.source() != controller_id
                    || !observation
                        .validates_workflow_tool(
                            observation.item(),
                            observation.run(),
                            controller_id,
                            payload,
                        )
                        .map_err(agent_error)?
                {
                    return invalid("Oracle effect projection changed its Controller source");
                }
                Ok(())
            }
            _ => invalid("Oracle effect projection is incomplete"),
        }
    }
}

impl TryFrom<WorkflowToolExecutedObservationWire> for WorkflowToolExecutedObservationV1 {
    type Error = WorkflowToolError;

    fn try_from(wire: WorkflowToolExecutedObservationWire) -> Result<Self, Self::Error> {
        let value = Self {
            controller: wire.controller,
            oracle_payload: wire.oracle_payload,
            oracle_observation: wire.oracle_observation,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for WorkflowToolExecutedObservationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        WorkflowToolExecutedObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Worker-side execution identity selected without performing the external effect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowToolWorkerBindingV1 {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
}

impl WorkflowToolWorkerBindingV1 {
    #[must_use]
    pub const fn new(
        job_id: JobId,
        attempt_id: AttemptId,
        contract_id: ContentId<JobContractArtifact>,
    ) -> Self {
        Self {
            job_id,
            attempt_id,
            contract_id,
        }
    }

    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }

    #[must_use]
    pub const fn attempt_id(&self) -> AttemptId {
        self.attempt_id
    }

    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }
}

/// Exact dispatch visible to a trusted Controller Worker adapter only after durable start authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkflowToolDispatchV1 {
    schema_version: SchemaVersion,
    worker_request: ContentId<WorkflowToolRequestArtifact>,
    operation_id: OperationId,
    tool_attempt_id: AttemptId,
    worker: WorkflowToolWorkerBindingV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowToolDispatchWire {
    schema_version: SchemaVersion,
    worker_request: ContentId<WorkflowToolRequestArtifact>,
    operation_id: OperationId,
    tool_attempt_id: AttemptId,
    worker: WorkflowToolWorkerBindingV1,
}

impl WorkflowToolDispatchV1 {
    #[must_use]
    pub const fn worker_request(&self) -> ContentId<WorkflowToolRequestArtifact> {
        self.worker_request
    }

    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    #[must_use]
    pub const fn tool_attempt_id(&self) -> AttemptId {
        self.tool_attempt_id
    }

    #[must_use]
    pub const fn worker(&self) -> &WorkflowToolWorkerBindingV1 {
        &self.worker
    }

    /// Derives the exact dispatch identity.
    ///
    /// # Errors
    ///
    /// Returns an error if current-V1 canonical encoding or content identity derivation fails.
    pub fn identity(&self) -> Result<ContentId<WorkflowToolDispatchArtifact>, WorkflowToolError> {
        self.validate()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec_error)?).map_err(codec_error)
    }

    fn validate(&self) -> Result<(), WorkflowToolError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal step Worker dispatch is not current V1");
        }
        Ok(())
    }
}

impl TryFrom<WorkflowToolDispatchWire> for WorkflowToolDispatchV1 {
    type Error = WorkflowToolError;

    fn try_from(wire: WorkflowToolDispatchWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            worker_request: wire.worker_request,
            operation_id: wire.operation_id,
            tool_attempt_id: wire.tool_attempt_id,
            worker: wire.worker,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for WorkflowToolDispatchV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        WorkflowToolDispatchWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Trusted Worker result returned to the Controller before it becomes a model-visible observation.
#[derive(Clone, Debug)]
pub struct WorkflowToolWorkerObservationV1 {
    dispatch: ContentId<WorkflowToolDispatchArtifact>,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

impl WorkflowToolWorkerObservationV1 {
    /// Binds one trusted Worker receipt and public observation to the exact dispatch.
    ///
    /// # Errors
    ///
    /// Rejects job, attempt, contract, dispatch, receipt identity, or codec drift.
    pub fn new(
        dispatch: &WorkflowToolDispatchV1,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: ExecutionReceipt,
        observation: serde_json::Value,
    ) -> Result<Self, WorkflowToolError> {
        if dispatch.schema_version != schema_v1()
            || receipt.job_id() != dispatch.worker.job_id
            || receipt.attempt_id() != dispatch.worker.attempt_id
            || receipt.contract_id() != dispatch.worker.contract_id
        {
            return invalid("Worker observation changed its authorized execution binding");
        }
        let bytes = cairn_codec::to_vec(&receipt).map_err(codec_error)?;
        if ContentId::<ExecutionReceiptArtifact>::derive(&bytes).map_err(codec_error)? != receipt_id
        {
            return invalid("Worker receipt changed its content identity");
        }
        Ok(Self {
            dispatch: dispatch.identity()?,
            receipt_id,
            receipt,
            observation,
        })
    }
}

/// Trusted adapter selected by Controller policy for one external tool implementation.
pub trait WorkflowToolWorker {
    /// Resolves an exact Worker job/attempt/contract without starting the external workload.
    ///
    /// # Errors
    ///
    /// Returns a classified error when the adapter cannot prepare an exact execution binding.
    fn prepare(
        &mut self,
        operation: &WorkflowToolOperationV1,
    ) -> Result<WorkflowToolWorkerBindingV1, WorkflowToolWorkerError>;

    /// Executes only after the Controller has committed durable tool-operation start authority.
    ///
    /// # Errors
    ///
    /// Returns a classified terminal or ambiguous Worker observation failure.
    fn execute(
        &mut self,
        dispatch: &WorkflowToolDispatchV1,
    ) -> Result<WorkflowToolWorkerObservationV1, WorkflowToolWorkerError>;
}

/// Worker failure classification preserved by the neutral agent operation state machine.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum WorkflowToolWorkerError {
    #[error("Worker proved the requested operation did not start: {0}")]
    NotStarted(String),
    #[error("Worker rejected the requested operation: {0}")]
    Rejected(String),
    #[error("Worker operation outcome is ambiguous: {0}")]
    Ambiguous(String),
}

/// Failure while validating, authorizing, executing, or publishing a Worker round trip.
#[derive(Debug, Error)]
pub enum WorkflowToolError {
    #[error("invalid proposal-step Worker request: {0}")]
    Invalid(String),
    #[error("proposal-step Worker request codec failed: {0}")]
    Codec(String),
    #[error("proposal-step Worker operation failed: {0}")]
    Agent(String),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceBearingObservationV1 {
    schema_version: SchemaVersion,
    controller_observation: ContentId<WorkflowToolControllerObservationArtifact>,
    oracle_observation: Option<ContentId<crate::OracleExplorationObservationArtifact>>,
    executed: WorkflowToolExecutedObservationV1,
}

impl ProvenanceBearingObservationV1 {
    fn new(executed: WorkflowToolExecutedObservationV1) -> Result<Self, WorkflowToolError> {
        executed.validate()?;
        let controller_observation = executed.controller.identity()?;
        let oracle_observation = executed
            .oracle_observation
            .as_ref()
            .map(OracleExplorationObservationV1::identity)
            .transpose()
            .map_err(agent_error)?;
        Ok(Self {
            schema_version: schema_v1(),
            controller_observation,
            oracle_observation,
            executed,
        })
    }

    fn validate(self) -> Result<WorkflowToolExecutedObservationV1, WorkflowToolError> {
        if self.schema_version != schema_v1()
            || self.executed.controller.identity()? != self.controller_observation
            || self
                .executed
                .oracle_observation
                .as_ref()
                .map(OracleExplorationObservationV1::identity)
                .transpose()
                .map_err(agent_error)?
                != self.oracle_observation
        {
            return invalid("model-visible effect result changed its typed observation identity");
        }
        self.executed.validate()?;
        Ok(self.executed)
    }
}

/// Controller validates a workflow tool request, grants durable start authority, invokes the selected Worker,
/// and records a receipt-bound observation into the same episode operation stream.
///
/// Calling the Proposal step again after this function resumes the exact episode and projects the
/// observation into its native continuation without another dispatch of the requesting model turn.
///
/// # Errors
///
/// Rejects workflow-tool/binding/receipt drift, missing authority, classified Worker failures, and
/// durable operation publication failures.
pub fn execute_workflow_tools<E, C, W>(
    events: &mut E,
    content: &mut C,
    workflow_request: &ProposalStepRequestV1,
    worker_request: &WorkflowToolRequestV1,
    worker: &mut W,
) -> Result<Vec<WorkflowToolExecutedObservationV1>, WorkflowToolError>
where
    E: EventStore,
    C: ContentStore,
    W: WorkflowToolWorker,
{
    worker_request
        .validate_against(workflow_request)
        .map_err(agent_error)?;
    let durable = recover_bound_operations(events, content, workflow_request, worker_request)?;
    let mut observations = Vec::with_capacity(worker_request.operations().len());
    for requested in worker_request.operations() {
        let operation = durable
            .iter()
            .find(|operation| operation.operation_id() == requested.operation_id())
            .ok_or_else(|| {
                WorkflowToolError::Invalid(
                    "requested operation is absent from the durable step binding".into(),
                )
            })?;
        validate_operation(operation, requested)?;
        observations.push(execute_one_experiment(
            events,
            content,
            workflow_request,
            worker_request,
            requested,
            operation.clone(),
            worker,
        )?);
    }
    Ok(observations)
}

fn recover_bound_operations<E: EventStore, C: ContentStore>(
    events: &E,
    content: &mut C,
    workflow_request: &ProposalStepRequestV1,
    worker_request: &WorkflowToolRequestV1,
) -> Result<Vec<PreparedToolOperation>, WorkflowToolError> {
    let episode =
        AgentEpisode::new(workflow_request.runtime().episode_id()).map_err(agent_error)?;
    let AgentEpisodeState::Active {
        step,
        model_attempt_id,
        step_state: AgentStepState::OperationsBound(bound),
    } = recover_agent_episode(events, content, &episode).map_err(agent_error)?
    else {
        return invalid("Proposal step episode is not waiting at a bound-operation safe point");
    };
    if step.step_id() != worker_request.step_id()
        || model_attempt_id != worker_request.model_attempt_id()
        || bound.operations().len() < worker_request.operations().len()
    {
        return invalid("proposal-step Worker request changed its step binding");
    }
    Ok(bound.into_operations())
}

fn validate_operation(
    durable: &PreparedToolOperation,
    requested: &WorkflowToolOperationV1,
) -> Result<(), WorkflowToolError> {
    if durable.tool() != requested.tool()
        || durable.implementation_version() != requested.implementation_version()
        || durable.effect() != requested.effect()
        || durable.arguments_id() != requested.arguments_id()
        || cairn_codec::from_slice::<serde_json::Value>(durable.argument_bytes())
            .map_err(codec_error)?
            != *requested.arguments()
    {
        return invalid("proposal-step Worker request differs from its operation binding");
    }
    Ok(())
}

fn execute_one_experiment<E, C, W>(
    events: &mut E,
    content: &mut C,
    workflow_request: &ProposalStepRequestV1,
    worker_request: &WorkflowToolRequestV1,
    requested: &WorkflowToolOperationV1,
    operation: PreparedToolOperation,
    worker: &mut W,
) -> Result<WorkflowToolExecutedObservationV1, WorkflowToolError>
where
    E: EventStore,
    C: ContentStore,
    W: WorkflowToolWorker,
{
    match recover_tool_operation(events, operation.operation_id()).map_err(agent_error)? {
        ToolOperationState::Completed { result_id, .. } => {
            return recover_executed_observation(content, result_id, workflow_request);
        }
        ToolOperationState::NotFound => {}
        _ => {
            return invalid("proposal-step Worker operation requires explicit reconciliation");
        }
    }
    let binding = worker
        .prepare(requested)
        .map_err(|error| worker_error(&error))?;
    let tool_attempt_id = AttemptId::new();
    let dispatch = WorkflowToolDispatchV1 {
        schema_version: schema_v1(),
        worker_request: worker_request.identity().map_err(agent_error)?,
        operation_id: operation.operation_id(),
        tool_attempt_id,
        worker: binding,
    };
    let authority = authorize_tool_operation(events, &CommandId::new(), observed_now()?, operation)
        .map_err(agent_error)?;
    let started = begin_tool_operation(
        events,
        authority,
        tool_attempt_id,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?;
    let mut gateway = WorkerGateway {
        worker,
        dispatch,
        workflow_request,
        executed: None,
    };
    let _ = execute_tool_operation(
        events,
        content,
        &mut gateway,
        started,
        &CommandId::new(),
        observed_now()?,
    )
    .map_err(agent_error)?;
    gateway
        .executed
        .ok_or_else(|| WorkflowToolError::Agent("Worker returned no observation".into()))
}

struct WorkerGateway<'a, W> {
    worker: &'a mut W,
    dispatch: WorkflowToolDispatchV1,
    workflow_request: &'a ProposalStepRequestV1,
    executed: Option<WorkflowToolExecutedObservationV1>,
}

impl<W: WorkflowToolWorker> ToolGateway for WorkerGateway<'_, W> {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        if operation.operation_id() != self.dispatch.operation_id {
            return Err(ToolGatewayError::Rejected(
                "Controller dispatch changed its durable operation".into(),
            ));
        }
        let observed = self
            .worker
            .execute(&self.dispatch)
            .map_err(|error| match error {
                WorkflowToolWorkerError::NotStarted(message) => {
                    ToolGatewayError::NotStarted(message)
                }
                WorkflowToolWorkerError::Rejected(message) => ToolGatewayError::Rejected(message),
                WorkflowToolWorkerError::Ambiguous(message) => ToolGatewayError::Ambiguous(message),
            })?;
        if observed.dispatch
            != self
                .dispatch
                .identity()
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?
        {
            return Err(ToolGatewayError::Rejected(
                "Worker observation changed its Controller dispatch".into(),
            ));
        }
        let controller =
            WorkflowToolControllerObservationV1::from_worker(self.dispatch.clone(), observed)
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let executed = project_observation(self.workflow_request, controller)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let envelope = ProvenanceBearingObservationV1::new(executed.clone())
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let value = serde_json::to_value(envelope)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        self.executed = Some(executed);
        CanonicalToolResult::from_value(&value)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
    }
}

fn project_observation(
    workflow_request: &ProposalStepRequestV1,
    controller: WorkflowToolControllerObservationV1,
) -> Result<WorkflowToolExecutedObservationV1, WorkflowToolError> {
    let (oracle_payload, oracle_observation) = match workflow_request.role() {
        ProposalStepRoleRequestV1::OracleStrategy { work_item, run, .. } => {
            let source = controller.identity()?;
            let payload = OracleObservationPayloadV1::new(source, controller.observation().clone());
            let observation = OracleExplorationObservationV1::workflow_tool(
                work_item.identity().map_err(agent_error)?,
                run.identity().map_err(agent_error)?,
                source,
                &payload,
            )
            .map_err(agent_error)?;
            (Some(payload), Some(observation))
        }
        _ => (None, None),
    };
    let executed = WorkflowToolExecutedObservationV1 {
        controller,
        oracle_payload,
        oracle_observation,
    };
    executed.validate()?;
    Ok(executed)
}

fn recover_executed_observation<C: ContentStore>(
    content: &C,
    result: ContentId<OperationResult>,
    workflow_request: &ProposalStepRequestV1,
) -> Result<WorkflowToolExecutedObservationV1, WorkflowToolError> {
    let mut bytes = Vec::new();
    content.write_to(&result, &mut bytes).map_err(agent_error)?;
    let envelope: ProvenanceBearingObservationV1 =
        cairn_codec::from_slice(&bytes).map_err(codec_error)?;
    if cairn_codec::to_vec(&envelope).map_err(codec_error)? != bytes {
        return invalid("persisted effect result is not canonical current V1");
    }
    let executed = envelope.validate()?;
    let expected = project_observation(workflow_request, executed.controller.clone())?;
    if executed != expected {
        return invalid("persisted effect result changed its workflow role projection");
    }
    Ok(executed)
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("schema version one is valid")
}

fn observed_now() -> Result<ObservedAtUnixMillis, WorkflowToolError> {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(agent_error)?
        .as_millis();
    Ok(ObservedAtUnixMillis::new(
        i64::try_from(milliseconds)
            .map_err(|_| WorkflowToolError::Agent("wall clock overflow".into()))?,
    ))
}

fn invalid<T>(message: &str) -> Result<T, WorkflowToolError> {
    Err(WorkflowToolError::Invalid(message.into()))
}

fn codec_error(error: impl std::fmt::Display) -> WorkflowToolError {
    WorkflowToolError::Codec(error.to_string())
}

fn agent_error(error: impl std::fmt::Display) -> WorkflowToolError {
    WorkflowToolError::Agent(error.to_string())
}

fn worker_error(error: &WorkflowToolWorkerError) -> WorkflowToolError {
    WorkflowToolError::Agent(error.to_string())
}

#[cfg(test)]
mod tests {
    use cairn_execution::{
        ExecutionEvidenceArtifact, ExecutionStderrArtifact, ExecutionStdoutArtifact,
    };

    use super::*;

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content identity")
    }

    fn receipt(
        binding: &WorkflowToolWorkerBindingV1,
    ) -> (ContentId<ExecutionReceiptArtifact>, ExecutionReceipt) {
        let bytes = cairn_codec::to_vec(&serde_json::json!({
            "schema_version":1,
            "job_id":binding.job_id(),
            "attempt_id":binding.attempt_id(),
            "contract_id":binding.contract_id(),
            "outcome":"succeeded",
            "exit_code":0,
            "elapsed_ms":7,
            "stdout_id":id::<ExecutionStdoutArtifact>(b"stdout"),
            "stderr_id":id::<ExecutionStderrArtifact>(b"stderr"),
            "evidence_id":id::<ExecutionEvidenceArtifact>(b"evidence"),
            "outputs":[]
        }))
        .expect("receipt bytes");
        let receipt = cairn_codec::from_slice(&bytes).expect("receipt");
        (
            ContentId::<ExecutionReceiptArtifact>::derive(&bytes).expect("receipt identity"),
            receipt,
        )
    }

    #[test]
    fn worker_observation_binds_dispatch_job_attempt_contract_and_receipt() {
        let binding = WorkflowToolWorkerBindingV1::new(
            JobId::new(),
            AttemptId::new(),
            id::<JobContractArtifact>(b"contract"),
        );
        let dispatch = WorkflowToolDispatchV1 {
            schema_version: schema_v1(),
            worker_request: id::<WorkflowToolRequestArtifact>(b"worker_request"),
            operation_id: OperationId::new(),
            tool_attempt_id: AttemptId::new(),
            worker: binding.clone(),
        };
        let (receipt_id, receipt) = receipt(&binding);
        WorkflowToolWorkerObservationV1::new(
            &dispatch,
            receipt_id,
            receipt.clone(),
            serde_json::json!({"measured":true}),
        )
        .expect("exact observation");

        let mut non_v1 = serde_json::to_value(&dispatch).expect("dispatch value");
        non_v1["schema_version"] = serde_json::json!(2);
        assert!(
            cairn_codec::from_slice::<WorkflowToolDispatchV1>(
                &cairn_codec::to_vec(&non_v1).expect("non-v1 bytes")
            )
            .is_err()
        );

        let changed = WorkflowToolWorkerBindingV1::new(
            JobId::new(),
            binding.attempt_id(),
            binding.contract_id(),
        );
        let changed_dispatch = WorkflowToolDispatchV1 {
            worker: changed,
            ..dispatch
        };
        assert!(
            WorkflowToolWorkerObservationV1::new(
                &changed_dispatch,
                receipt_id,
                receipt,
                serde_json::json!({"measured":true}),
            )
            .is_err()
        );
    }
}
