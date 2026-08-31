//! Controller-owned bridge from a Proposal Host durable yield to a managed Worker observation.

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
    OracleExplorationObservationV1, OracleObservationPayloadV1,
    ProposalHostControllerObservationArtifact, ProposalHostExperimentOperationV1,
    ProposalHostExperimentRequestArtifact, ProposalHostExperimentRequestV1, ProposalHostRequestV1,
    ProposalHostRoleRequestV1,
};

/// Identity of one exact Controller-authorized experiment dispatch.
pub enum ProposalHostExperimentDispatchArtifact {}

impl ContentType for ProposalHostExperimentDispatchArtifact {
    const DOMAIN: &'static str = "migration.proposal-host-experiment-dispatch.v1";
}

/// Controller-recomputed effect observation returned to the same Host episode.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostControllerObservationV1 {
    schema_version: SchemaVersion,
    dispatch: ProposalHostExperimentDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostControllerObservationWire {
    schema_version: SchemaVersion,
    dispatch: ProposalHostExperimentDispatchV1,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

impl ProposalHostControllerObservationV1 {
    fn from_worker(
        dispatch: ProposalHostExperimentDispatchV1,
        observed: ProposalHostWorkerObservationV1,
    ) -> Result<Self, ProposalHostExperimentError> {
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
    pub const fn dispatch(&self) -> &ProposalHostExperimentDispatchV1 {
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
    ) -> Result<ContentId<ProposalHostControllerObservationArtifact>, ProposalHostExperimentError>
    {
        self.validate()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec_error)?).map_err(codec_error)
    }

    fn validate(&self) -> Result<(), ProposalHostExperimentError> {
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

impl TryFrom<ProposalHostControllerObservationWire> for ProposalHostControllerObservationV1 {
    type Error = ProposalHostExperimentError;

    fn try_from(wire: ProposalHostControllerObservationWire) -> Result<Self, Self::Error> {
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

impl<'de> Deserialize<'de> for ProposalHostControllerObservationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProposalHostControllerObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Exact Controller effect plus its optional Oracle-domain projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProposalHostExecutedObservationV1 {
    controller: ProposalHostControllerObservationV1,
    oracle_payload: Option<OracleObservationPayloadV1>,
    oracle_observation: Option<OracleExplorationObservationV1>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostExecutedObservationWire {
    controller: ProposalHostControllerObservationV1,
    oracle_payload: Option<OracleObservationPayloadV1>,
    oracle_observation: Option<OracleExplorationObservationV1>,
}

impl ProposalHostExecutedObservationV1 {
    #[must_use]
    pub const fn controller(&self) -> &ProposalHostControllerObservationV1 {
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

    fn validate(&self) -> Result<(), ProposalHostExperimentError> {
        self.controller.validate()?;
        match (&self.oracle_payload, &self.oracle_observation) {
            (None, None) => Ok(()),
            (Some(payload), Some(observation)) => {
                let controller_id = self.controller.identity()?;
                if payload.source() != controller_id
                    || !observation
                        .validates_proposal_host_effect(
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

impl TryFrom<ProposalHostExecutedObservationWire> for ProposalHostExecutedObservationV1 {
    type Error = ProposalHostExperimentError;

    fn try_from(wire: ProposalHostExecutedObservationWire) -> Result<Self, Self::Error> {
        let value = Self {
            controller: wire.controller,
            oracle_payload: wire.oracle_payload,
            oracle_observation: wire.oracle_observation,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostExecutedObservationV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProposalHostExecutedObservationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Worker-side execution identity selected without performing the external effect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalHostWorkerBindingV1 {
    job_id: JobId,
    attempt_id: AttemptId,
    contract_id: ContentId<JobContractArtifact>,
}

impl ProposalHostWorkerBindingV1 {
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
pub struct ProposalHostExperimentDispatchV1 {
    schema_version: SchemaVersion,
    experiment: ContentId<ProposalHostExperimentRequestArtifact>,
    operation_id: OperationId,
    tool_attempt_id: AttemptId,
    worker: ProposalHostWorkerBindingV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalHostExperimentDispatchWire {
    schema_version: SchemaVersion,
    experiment: ContentId<ProposalHostExperimentRequestArtifact>,
    operation_id: OperationId,
    tool_attempt_id: AttemptId,
    worker: ProposalHostWorkerBindingV1,
}

impl ProposalHostExperimentDispatchV1 {
    #[must_use]
    pub const fn experiment(&self) -> ContentId<ProposalHostExperimentRequestArtifact> {
        self.experiment
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
    pub const fn worker(&self) -> &ProposalHostWorkerBindingV1 {
        &self.worker
    }

    /// Derives the exact dispatch identity.
    ///
    /// # Errors
    ///
    /// Returns an error if current-V1 canonical encoding or content identity derivation fails.
    pub fn identity(
        &self,
    ) -> Result<ContentId<ProposalHostExperimentDispatchArtifact>, ProposalHostExperimentError>
    {
        self.validate()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec_error)?).map_err(codec_error)
    }

    fn validate(&self) -> Result<(), ProposalHostExperimentError> {
        if self.schema_version != schema_v1() {
            return invalid("Proposal Host experiment dispatch is not current V1");
        }
        Ok(())
    }
}

impl TryFrom<ProposalHostExperimentDispatchWire> for ProposalHostExperimentDispatchV1 {
    type Error = ProposalHostExperimentError;

    fn try_from(wire: ProposalHostExperimentDispatchWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            experiment: wire.experiment,
            operation_id: wire.operation_id,
            tool_attempt_id: wire.tool_attempt_id,
            worker: wire.worker,
        };
        value.validate()?;
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for ProposalHostExperimentDispatchV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ProposalHostExperimentDispatchWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Trusted Worker result returned to the Controller before it becomes a model-visible observation.
#[derive(Clone, Debug)]
pub struct ProposalHostWorkerObservationV1 {
    dispatch: ContentId<ProposalHostExperimentDispatchArtifact>,
    receipt_id: ContentId<ExecutionReceiptArtifact>,
    receipt: ExecutionReceipt,
    observation: serde_json::Value,
}

impl ProposalHostWorkerObservationV1 {
    /// Binds one trusted Worker receipt and public observation to the exact dispatch.
    ///
    /// # Errors
    ///
    /// Rejects job, attempt, contract, dispatch, receipt identity, or codec drift.
    pub fn new(
        dispatch: &ProposalHostExperimentDispatchV1,
        receipt_id: ContentId<ExecutionReceiptArtifact>,
        receipt: ExecutionReceipt,
        observation: serde_json::Value,
    ) -> Result<Self, ProposalHostExperimentError> {
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
pub trait ProposalHostExperimentWorker {
    /// Resolves an exact Worker job/attempt/contract without starting the external workload.
    ///
    /// # Errors
    ///
    /// Returns a classified error when the adapter cannot prepare an exact execution binding.
    fn prepare(
        &mut self,
        operation: &ProposalHostExperimentOperationV1,
    ) -> Result<ProposalHostWorkerBindingV1, ProposalHostExperimentWorkerError>;

    /// Executes only after the Controller has committed durable tool-operation start authority.
    ///
    /// # Errors
    ///
    /// Returns a classified terminal or ambiguous Worker observation failure.
    fn execute(
        &mut self,
        dispatch: &ProposalHostExperimentDispatchV1,
    ) -> Result<ProposalHostWorkerObservationV1, ProposalHostExperimentWorkerError>;
}

/// Worker failure classification preserved by the neutral agent operation state machine.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProposalHostExperimentWorkerError {
    #[error("Worker proved the experiment did not start: {0}")]
    NotStarted(String),
    #[error("Worker rejected the experiment: {0}")]
    Rejected(String),
    #[error("Worker experiment outcome is ambiguous: {0}")]
    Ambiguous(String),
}

/// Failure while validating, authorizing, executing, or publishing an experiment round trip.
#[derive(Debug, Error)]
pub enum ProposalHostExperimentError {
    #[error("invalid Proposal Host experiment: {0}")]
    Invalid(String),
    #[error("Proposal Host experiment codec failed: {0}")]
    Codec(String),
    #[error("Proposal Host experiment durable operation failed: {0}")]
    Agent(String),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceBearingObservationV1 {
    schema_version: SchemaVersion,
    controller_observation: ContentId<ProposalHostControllerObservationArtifact>,
    oracle_observation: Option<ContentId<crate::OracleExplorationObservationArtifact>>,
    executed: ProposalHostExecutedObservationV1,
}

impl ProvenanceBearingObservationV1 {
    fn new(
        executed: ProposalHostExecutedObservationV1,
    ) -> Result<Self, ProposalHostExperimentError> {
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

    fn validate(self) -> Result<ProposalHostExecutedObservationV1, ProposalHostExperimentError> {
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

/// Controller validates a Host yield, grants durable start authority, invokes the selected Worker,
/// and records a receipt-bound observation into the same episode operation stream.
///
/// Calling the Proposal Host again after this function resumes the exact episode and projects the
/// observation into its native continuation without another dispatch of the yielding model turn.
///
/// # Errors
///
/// Rejects Host/yield/binding/receipt drift, missing authority, classified Worker failures, and
/// durable operation publication failures.
pub fn execute_proposal_host_experiments<E, C, W>(
    events: &mut E,
    content: &mut C,
    host_request: &ProposalHostRequestV1,
    experiment: &ProposalHostExperimentRequestV1,
    worker: &mut W,
) -> Result<Vec<ProposalHostExecutedObservationV1>, ProposalHostExperimentError>
where
    E: EventStore,
    C: ContentStore,
    W: ProposalHostExperimentWorker,
{
    experiment
        .validate_against(host_request)
        .map_err(agent_error)?;
    let durable = recover_bound_operations(events, content, host_request, experiment)?;
    let mut observations = Vec::with_capacity(experiment.operations().len());
    for yielded in experiment.operations() {
        let operation = durable
            .iter()
            .find(|operation| operation.operation_id() == yielded.operation_id())
            .ok_or_else(|| {
                ProposalHostExperimentError::Invalid(
                    "yielded operation is absent from the durable step binding".into(),
                )
            })?;
        validate_operation(operation, yielded)?;
        observations.push(execute_one_experiment(
            events,
            content,
            host_request,
            experiment,
            yielded,
            operation.clone(),
            worker,
        )?);
    }
    Ok(observations)
}

fn recover_bound_operations<E: EventStore, C: ContentStore>(
    events: &E,
    content: &mut C,
    host_request: &ProposalHostRequestV1,
    experiment: &ProposalHostExperimentRequestV1,
) -> Result<Vec<PreparedToolOperation>, ProposalHostExperimentError> {
    let episode = AgentEpisode::new(host_request.runtime().episode_id()).map_err(agent_error)?;
    let AgentEpisodeState::Active {
        step,
        model_attempt_id,
        step_state: AgentStepState::OperationsBound(bound),
    } = recover_agent_episode(events, content, &episode).map_err(agent_error)?
    else {
        return invalid("Proposal Host episode is not waiting at a bound-operation safe point");
    };
    if step.step_id() != experiment.step_id()
        || model_attempt_id != experiment.model_attempt_id()
        || bound.operations().len() < experiment.operations().len()
    {
        return invalid("Proposal Host experiment changed its durable step binding");
    }
    Ok(bound.into_operations())
}

fn validate_operation(
    durable: &PreparedToolOperation,
    yielded: &ProposalHostExperimentOperationV1,
) -> Result<(), ProposalHostExperimentError> {
    if durable.tool() != yielded.tool()
        || durable.implementation_version() != yielded.implementation_version()
        || durable.effect() != yielded.effect()
        || durable.arguments_id() != yielded.arguments_id()
        || cairn_codec::from_slice::<serde_json::Value>(durable.argument_bytes())
            .map_err(codec_error)?
            != *yielded.arguments()
    {
        return invalid("Proposal Host experiment differs from its durable operation binding");
    }
    Ok(())
}

fn execute_one_experiment<E, C, W>(
    events: &mut E,
    content: &mut C,
    host_request: &ProposalHostRequestV1,
    experiment: &ProposalHostExperimentRequestV1,
    yielded: &ProposalHostExperimentOperationV1,
    operation: PreparedToolOperation,
    worker: &mut W,
) -> Result<ProposalHostExecutedObservationV1, ProposalHostExperimentError>
where
    E: EventStore,
    C: ContentStore,
    W: ProposalHostExperimentWorker,
{
    match recover_tool_operation(events, operation.operation_id()).map_err(agent_error)? {
        ToolOperationState::Completed { result_id, .. } => {
            return recover_executed_observation(content, result_id, host_request);
        }
        ToolOperationState::NotFound => {}
        _ => return invalid("Proposal Host experiment operation requires explicit reconciliation"),
    }
    let binding = worker
        .prepare(yielded)
        .map_err(|error| worker_error(&error))?;
    let tool_attempt_id = AttemptId::new();
    let dispatch = ProposalHostExperimentDispatchV1 {
        schema_version: schema_v1(),
        experiment: experiment.identity().map_err(agent_error)?,
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
        host_request,
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
        .ok_or_else(|| ProposalHostExperimentError::Agent("Worker returned no observation".into()))
}

struct WorkerGateway<'a, W> {
    worker: &'a mut W,
    dispatch: ProposalHostExperimentDispatchV1,
    host_request: &'a ProposalHostRequestV1,
    executed: Option<ProposalHostExecutedObservationV1>,
}

impl<W: ProposalHostExperimentWorker> ToolGateway for WorkerGateway<'_, W> {
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
                ProposalHostExperimentWorkerError::NotStarted(message) => {
                    ToolGatewayError::NotStarted(message)
                }
                ProposalHostExperimentWorkerError::Rejected(message) => {
                    ToolGatewayError::Rejected(message)
                }
                ProposalHostExperimentWorkerError::Ambiguous(message) => {
                    ToolGatewayError::Ambiguous(message)
                }
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
            ProposalHostControllerObservationV1::from_worker(self.dispatch.clone(), observed)
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let executed = project_observation(self.host_request, controller)
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
    host_request: &ProposalHostRequestV1,
    controller: ProposalHostControllerObservationV1,
) -> Result<ProposalHostExecutedObservationV1, ProposalHostExperimentError> {
    let (oracle_payload, oracle_observation) = match host_request.role() {
        ProposalHostRoleRequestV1::OracleStrategy { work_item, run, .. } => {
            let source = controller.identity()?;
            let payload = OracleObservationPayloadV1::new(source, controller.observation().clone());
            let observation = OracleExplorationObservationV1::proposal_host_effect(
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
    let executed = ProposalHostExecutedObservationV1 {
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
    host_request: &ProposalHostRequestV1,
) -> Result<ProposalHostExecutedObservationV1, ProposalHostExperimentError> {
    let mut bytes = Vec::new();
    content.write_to(&result, &mut bytes).map_err(agent_error)?;
    let envelope: ProvenanceBearingObservationV1 =
        cairn_codec::from_slice(&bytes).map_err(codec_error)?;
    if cairn_codec::to_vec(&envelope).map_err(codec_error)? != bytes {
        return invalid("persisted effect result is not canonical current V1");
    }
    let executed = envelope.validate()?;
    let expected = project_observation(host_request, executed.controller.clone())?;
    if executed != expected {
        return invalid("persisted effect result changed its Host role projection");
    }
    Ok(executed)
}

fn schema_v1() -> SchemaVersion {
    SchemaVersion::new(1).expect("schema version one is valid")
}

fn observed_now() -> Result<ObservedAtUnixMillis, ProposalHostExperimentError> {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(agent_error)?
        .as_millis();
    Ok(ObservedAtUnixMillis::new(
        i64::try_from(milliseconds)
            .map_err(|_| ProposalHostExperimentError::Agent("wall clock overflow".into()))?,
    ))
}

fn invalid<T>(message: &str) -> Result<T, ProposalHostExperimentError> {
    Err(ProposalHostExperimentError::Invalid(message.into()))
}

fn codec_error(error: impl std::fmt::Display) -> ProposalHostExperimentError {
    ProposalHostExperimentError::Codec(error.to_string())
}

fn agent_error(error: impl std::fmt::Display) -> ProposalHostExperimentError {
    ProposalHostExperimentError::Agent(error.to_string())
}

fn worker_error(error: &ProposalHostExperimentWorkerError) -> ProposalHostExperimentError {
    ProposalHostExperimentError::Agent(error.to_string())
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
        binding: &ProposalHostWorkerBindingV1,
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
        let binding = ProposalHostWorkerBindingV1::new(
            JobId::new(),
            AttemptId::new(),
            id::<JobContractArtifact>(b"contract"),
        );
        let dispatch = ProposalHostExperimentDispatchV1 {
            schema_version: schema_v1(),
            experiment: id::<ProposalHostExperimentRequestArtifact>(b"experiment"),
            operation_id: OperationId::new(),
            tool_attempt_id: AttemptId::new(),
            worker: binding.clone(),
        };
        let (receipt_id, receipt) = receipt(&binding);
        ProposalHostWorkerObservationV1::new(
            &dispatch,
            receipt_id,
            receipt.clone(),
            serde_json::json!({"measured":true}),
        )
        .expect("exact observation");

        let mut non_v1 = serde_json::to_value(&dispatch).expect("dispatch value");
        non_v1["schema_version"] = serde_json::json!(2);
        assert!(
            cairn_codec::from_slice::<ProposalHostExperimentDispatchV1>(
                &cairn_codec::to_vec(&non_v1).expect("non-v1 bytes")
            )
            .is_err()
        );

        let changed = ProposalHostWorkerBindingV1::new(
            JobId::new(),
            binding.attempt_id(),
            binding.contract_id(),
        );
        let changed_dispatch = ProposalHostExperimentDispatchV1 {
            worker: changed,
            ..dispatch
        };
        assert!(
            ProposalHostWorkerObservationV1::new(
                &changed_dispatch,
                receipt_id,
                receipt,
                serde_json::json!({"measured":true}),
            )
            .is_err()
        );
    }
}
