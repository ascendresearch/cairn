use std::{collections::HashSet, io::Cursor};

use cairn_protocol::{
    AttemptId, CommandId, ContentId, EventId, ObservedAtUnixMillis, OperationId, SchemaName,
    SchemaVersion, StepId,
};
use cairn_record::{ContentStore, EventEnvelope, EventStore, ExpectedRevision, NewEvent};
use serde::{Deserialize, Serialize};

use crate::{
    AgentStep, AgentStepState, OperationRecovery, OperationResult, PreparedToolOperation,
    SemanticModelTurnArtifact, StepCoordinatorError, ToolCallId, ToolCallProposal, ToolEffectClass,
    ToolImplementationVersion, ToolName, ToolOperationState, recover_agent_step,
    recover_tool_operation,
};

pub(crate) const STEP_OPERATION_BOUND: &str = "agent.step-operation-bound";
pub(crate) const STEP_OPERATIONS_SETTLED: &str = "agent.step-operations-settled";
const STEP_SETTLED: &str = "agent.step-settled";

/// Trusted registry metadata used to turn a model proposal into an executable operation.
///
/// The model can request a tool name, but cannot construct or override the implementation version
/// or effect class held by this type.
///
/// ```compile_fail
/// use cairn_agent::ToolRegistration;
///
/// let forged = ToolRegistration {
///     name: todo!(),
///     implementation_version: todo!(),
///     effect: todo!(),
/// };
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRegistration {
    name: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
}

impl ToolRegistration {
    /// Creates one trusted runtime registration.
    #[must_use]
    pub const fn new(
        name: ToolName,
        implementation_version: ToolImplementationVersion,
        effect: ToolEffectClass,
    ) -> Self {
        Self {
            name,
            implementation_version,
            effect,
        }
    }

    /// Returns the registered tool name.
    #[must_use]
    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// Returns the pinned implementation version.
    #[must_use]
    pub fn implementation_version(&self) -> &ToolImplementationVersion {
        &self.implementation_version
    }

    /// Returns trusted external-effect semantics.
    #[must_use]
    pub const fn effect(&self) -> ToolEffectClass {
        self.effect
    }
}

/// Runtime assignment of one stable logical operation identity to one ordered proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolOperationAssignment {
    operation_id: OperationId,
    registration: ToolRegistration,
}

impl ToolOperationAssignment {
    /// Creates an assignment. Validation against the proposal happens atomically at binding time.
    #[must_use]
    pub const fn new(operation_id: OperationId, registration: ToolRegistration) -> Self {
        Self {
            operation_id,
            registration,
        }
    }

    /// Returns the stable logical operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Returns the trusted registration selected by the runtime.
    #[must_use]
    pub const fn registration(&self) -> &ToolRegistration {
        &self.registration
    }
}

/// Durable ordered bindings reconstructed with verified argument bytes.
#[derive(Debug)]
pub struct BoundStepOperations {
    turn_id: ContentId<SemanticModelTurnArtifact>,
    operations: Vec<PreparedToolOperation>,
}

/// Non-authoritative identity and trusted registration of a bound logical operation.
///
/// This projection deliberately excludes argument bytes and cannot grant execution authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepOperationIdentity {
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
}

impl StepOperationIdentity {
    /// Returns the stable logical operation identity.
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        self.operation_id
    }

    /// Returns the trusted registered tool name.
    #[must_use]
    pub fn tool(&self) -> &ToolName {
        &self.tool
    }

    /// Returns the pinned tool implementation version.
    #[must_use]
    pub fn implementation_version(&self) -> &ToolImplementationVersion {
        &self.implementation_version
    }

    /// Returns trusted external-effect semantics.
    #[must_use]
    pub const fn effect(&self) -> ToolEffectClass {
        self.effect
    }

    fn from_prepared(operation: &PreparedToolOperation) -> Self {
        Self {
            operation_id: operation.operation_id(),
            tool: operation.tool().clone(),
            implementation_version: operation.implementation_version().clone(),
            effect: operation.effect(),
        }
    }
}

impl BoundStepOperations {
    /// Returns the semantic turn that proposed these operations.
    #[must_use]
    pub const fn turn_id(&self) -> ContentId<SemanticModelTurnArtifact> {
        self.turn_id
    }

    /// Borrows prepared operations in model output order.
    #[must_use]
    pub fn operations(&self) -> &[PreparedToolOperation] {
        &self.operations
    }

    /// Consumes the binding projection into independently authorizable operations.
    #[must_use]
    pub fn into_operations(self) -> Vec<PreparedToolOperation> {
        self.operations
    }
}

/// Why a bound step cannot yet feed a subsequent model request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepOperationBlocker {
    /// The binding is durable but the operation aggregate has not been authorized.
    AwaitingAuthorization { operation_id: OperationId },
    /// Operation authority exists, but no invocation attempt has started.
    AwaitingAttempt { operation_id: OperationId },
    /// A concrete attempt is in doubt and runtime policy must explicitly retry it.
    RetryRequired {
        operation_id: OperationId,
        attempt_id: AttemptId,
        diagnostic: Option<String>,
    },
    /// External evidence or caller authority is required before progress.
    ReconcileRequired {
        operation_id: OperationId,
        attempt_id: AttemptId,
        diagnostic: Option<String>,
    },
}

/// Result of trying to close the operation boundary of a step.
#[derive(Debug)]
pub enum StepOperationSettlement {
    /// Every operation has a model-visible result in original proposal order.
    ReadyForNextStep {
        turn_id: ContentId<SemanticModelTurnArtifact>,
        pending_results: Vec<ContentId<OperationResult>>,
    },
    /// At least one operation still needs execution, retry, or reconciliation.
    Blocked {
        bound: BoundStepOperations,
        blockers: Vec<StepOperationBlocker>,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BindingPayload {
    step_id: StepId,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    tool_call_id: ToolCallId,
    operation_id: OperationId,
    tool: ToolName,
    implementation_version: ToolImplementationVersion,
    effect: ToolEffectClass,
    arguments_id: ContentId<crate::ToolArguments>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ResolvedOutcome {
    Completed,
    Rejected,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResolvedOperation {
    tool_call_id: ToolCallId,
    operation_id: OperationId,
    attempt_id: AttemptId,
    outcome: ResolvedOutcome,
    result_id: ContentId<OperationResult>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationsSettledPayload {
    step_id: StepId,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    results: Vec<ResolvedOperation>,
}

pub(crate) enum StepOperationProjection {
    Unbound(Vec<ToolCallProposal>),
    Bound(BoundStepOperations),
    Ready {
        turn_id: ContentId<SemanticModelTurnArtifact>,
        pending_results: Vec<ContentId<OperationResult>>,
        operations: Vec<StepOperationIdentity>,
    },
}

/// Atomically binds every ordered proposal to a unique operation and trusted registration.
///
/// No operation authority is granted here. A crash after this commit recovers the exact same
/// [`PreparedToolOperation`] values, which can then be authorized independently.
///
/// # Errors
///
/// Returns [`StepCoordinatorError`] for mismatched registrations, duplicate identities, invalid
/// durable facts, or storage failure.
pub fn bind_step_operations<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    step: &AgentStep,
    attempt_id: cairn_protocol::ModelAttemptId,
    assignments: Vec<ToolOperationAssignment>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<BoundStepOperations, StepCoordinatorError> {
    match recover_agent_step(events, content, step, attempt_id)? {
        AgentStepState::AwaitingOperations { turn_id, proposals } => bind_unbound(
            events,
            step,
            turn_id,
            proposals,
            assignments,
            command_id,
            observed_at,
        ),
        AgentStepState::OperationsBound(bound) => {
            validate_replayed_assignments(&bound, &assignments)?;
            Ok(bound)
        }
        _ => invalid_step("step is not awaiting tool-operation binding"),
    }
}

fn bind_unbound<E: EventStore>(
    events: &mut E,
    step: &AgentStep,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    proposals: Vec<ToolCallProposal>,
    assignments: Vec<ToolOperationAssignment>,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<BoundStepOperations, StepCoordinatorError> {
    if proposals.len() != assignments.len() {
        return invalid_step("every proposal must have exactly one operation assignment");
    }
    let history = events.read_stream(step.stream_id(), None)?;
    let settled_event_id = unique_event_id(&history, STEP_SETTLED)?;
    let last = history
        .last()
        .ok_or_else(|| StepCoordinatorError::InvalidStep("step history is empty".into()))?;
    let mut operation_ids = HashSet::new();
    let mut facts = Vec::with_capacity(proposals.len());
    let mut operations = Vec::with_capacity(proposals.len());
    for (proposal, assignment) in proposals.into_iter().zip(assignments) {
        if proposal.tool() != assignment.registration.name() {
            return invalid_step("trusted registration does not match the proposed tool name");
        }
        if !operation_ids.insert(assignment.operation_id.to_string()) {
            return invalid_step("one operation identity cannot bind multiple tool calls");
        }
        let payload = BindingPayload {
            step_id: step.step_id(),
            turn_id,
            tool_call_id: proposal.tool_call_id(),
            operation_id: assignment.operation_id,
            tool: proposal.tool().clone(),
            implementation_version: assignment.registration.implementation_version().clone(),
            effect: assignment.registration.effect(),
            arguments_id: proposal.arguments_id(),
        };
        facts.push(new_step_fact(
            STEP_OPERATION_BOUND,
            settled_event_id,
            observed_at,
            &payload,
        )?);
        operations.push(proposal.into_prepared_operation(
            assignment.operation_id,
            assignment.registration.implementation_version().clone(),
            assignment.registration.effect(),
        ));
    }
    events.append(
        step.stream_id(),
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        ),
        command_id,
        &facts,
    )?;
    Ok(BoundStepOperations {
        turn_id,
        operations,
    })
}

fn validate_replayed_assignments(
    bound: &BoundStepOperations,
    assignments: &[ToolOperationAssignment],
) -> Result<(), StepCoordinatorError> {
    if bound.operations.len() != assignments.len()
        || bound
            .operations
            .iter()
            .zip(assignments)
            .any(|(operation, assignment)| {
                operation.operation_id() != assignment.operation_id
                    || operation.tool() != assignment.registration.name()
                    || operation.implementation_version()
                        != assignment.registration.implementation_version()
                    || operation.effect() != assignment.registration.effect()
            })
    {
        return invalid_step("replayed assignments differ from durable operation bindings");
    }
    Ok(())
}

/// Collects terminal operation outcomes and commits an ordered next-step input boundary.
///
/// Definitive tool rejection becomes canonical model-visible feedback. Unknown or retryable
/// outcomes remain typed blockers and never masquerade as ordinary tool results.
///
/// # Errors
///
/// Returns [`StepCoordinatorError`] when durable state is invalid or storage fails.
pub fn settle_step_operations<E: EventStore, C: ContentStore>(
    events: &mut E,
    content: &mut C,
    step: &AgentStep,
    attempt_id: cairn_protocol::ModelAttemptId,
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<StepOperationSettlement, StepCoordinatorError> {
    let bound = match recover_agent_step(events, content, step, attempt_id)? {
        AgentStepState::OperationsBound(bound) => bound,
        AgentStepState::ReadyForNextStep {
            turn_id,
            pending_results,
            ..
        } => {
            return Ok(StepOperationSettlement::ReadyForNextStep {
                turn_id,
                pending_results,
            });
        }
        _ => return invalid_step("step has no complete durable operation bindings"),
    };
    let mut blockers = Vec::new();
    let mut resolved = Vec::with_capacity(bound.operations.len());
    for operation in &bound.operations {
        let operation_id = operation.operation_id();
        let tool_call_id = operation.source_tool_call_id().ok_or_else(|| {
            StepCoordinatorError::InvalidStep("bound operation has no source tool call".into())
        })?;
        match recover_tool_operation(events, operation_id)? {
            ToolOperationState::NotFound => {
                blockers.push(StepOperationBlocker::AwaitingAuthorization { operation_id });
            }
            ToolOperationState::Authorized { .. } => {
                blockers.push(StepOperationBlocker::AwaitingAttempt { operation_id });
            }
            ToolOperationState::Interrupted {
                attempt_id,
                recovery,
                ..
            } => blockers.push(recovery_blocker(operation_id, attempt_id, recovery, None)),
            ToolOperationState::NotStarted {
                attempt_id,
                diagnostic,
            } => blockers.push(StepOperationBlocker::RetryRequired {
                operation_id,
                attempt_id,
                diagnostic: Some(diagnostic),
            }),
            ToolOperationState::Ambiguous {
                attempt_id,
                recovery,
                diagnostic,
            } => blockers.push(recovery_blocker(
                operation_id,
                attempt_id,
                recovery,
                Some(diagnostic),
            )),
            ToolOperationState::Completed {
                attempt_id,
                result_id,
            } => resolved.push(ResolvedOperation {
                tool_call_id,
                operation_id,
                attempt_id,
                outcome: ResolvedOutcome::Completed,
                result_id,
            }),
            ToolOperationState::Rejected {
                attempt_id,
                diagnostic,
            } => {
                let bytes =
                    rejected_result_bytes(tool_call_id, operation_id, attempt_id, &diagnostic)?;
                let result_id = content
                    .put::<OperationResult>(&mut Cursor::new(bytes))?
                    .content_id;
                resolved.push(ResolvedOperation {
                    tool_call_id,
                    operation_id,
                    attempt_id,
                    outcome: ResolvedOutcome::Rejected,
                    result_id,
                });
            }
        }
    }
    if !blockers.is_empty() {
        return Ok(StepOperationSettlement::Blocked { bound, blockers });
    }
    commit_operation_results(
        events,
        step,
        bound.turn_id,
        &resolved,
        command_id,
        observed_at,
    )?;
    Ok(StepOperationSettlement::ReadyForNextStep {
        turn_id: bound.turn_id,
        pending_results: resolved.iter().map(|result| result.result_id).collect(),
    })
}

fn recovery_blocker(
    operation_id: OperationId,
    attempt_id: AttemptId,
    recovery: OperationRecovery,
    diagnostic: Option<String>,
) -> StepOperationBlocker {
    match recovery {
        OperationRecovery::RetrySameOperation => StepOperationBlocker::RetryRequired {
            operation_id,
            attempt_id,
            diagnostic,
        },
        OperationRecovery::ReconcileRequired => StepOperationBlocker::ReconcileRequired {
            operation_id,
            attempt_id,
            diagnostic,
        },
    }
}

fn commit_operation_results<E: EventStore>(
    events: &mut E,
    step: &AgentStep,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    resolved: &[ResolvedOperation],
    command_id: &CommandId,
    observed_at: ObservedAtUnixMillis,
) -> Result<(), StepCoordinatorError> {
    let history = events.read_stream(step.stream_id(), None)?;
    let last = history
        .last()
        .ok_or_else(|| StepCoordinatorError::InvalidStep("step history is empty".into()))?;
    let binding_parent = history
        .iter()
        .rfind(|event| event.schema_name.as_str() == STEP_OPERATION_BOUND)
        .map(|event| event.event_id)
        .ok_or_else(|| {
            StepCoordinatorError::InvalidStep("operation bindings are missing".into())
        })?;
    let payload = OperationsSettledPayload {
        step_id: step.step_id(),
        turn_id,
        results: resolved.to_vec(),
    };
    let event = new_step_fact(
        STEP_OPERATIONS_SETTLED,
        binding_parent,
        observed_at,
        &payload,
    )?;
    events.append(
        step.stream_id(),
        ExpectedRevision::Exact(
            cairn_protocol::StreamRevision::new(last.sequence.get())
                .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        ),
        command_id,
        &[event],
    )?;
    Ok(())
}

pub(crate) fn project_step_operations<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    history: &[EventEnvelope],
    step: &AgentStep,
    turn_id: ContentId<SemanticModelTurnArtifact>,
    proposals: Vec<ToolCallProposal>,
) -> Result<StepOperationProjection, StepCoordinatorError> {
    let settled_event_id = unique_event_id(history, STEP_SETTLED)?;
    let binding_facts: Vec<_> = history
        .iter()
        .filter(|event| event.schema_name.as_str() == STEP_OPERATION_BOUND)
        .collect();
    if binding_facts.is_empty() {
        if history
            .iter()
            .any(|event| event.schema_name.as_str() == STEP_OPERATIONS_SETTLED)
        {
            return invalid_step("operation results exist without operation bindings");
        }
        return Ok(StepOperationProjection::Unbound(proposals));
    }
    if binding_facts.len() != proposals.len() {
        return invalid_step("operation binding batch is incomplete or duplicated");
    }
    let mut operation_ids = HashSet::new();
    let mut operations = Vec::with_capacity(proposals.len());
    for (event, proposal) in binding_facts.into_iter().zip(proposals) {
        require_schema_v1(event)?;
        if event.parent_event_id != Some(settled_event_id) {
            return invalid_step("operation binding does not cite the settled step");
        }
        let payload: BindingPayload = decode_step_payload(event)?;
        if payload.step_id != step.step_id()
            || payload.turn_id != turn_id
            || payload.tool_call_id != proposal.tool_call_id()
            || payload.tool != *proposal.tool()
            || payload.arguments_id != proposal.arguments_id()
        {
            return invalid_step("operation binding differs from decoded tool proposal");
        }
        if !operation_ids.insert(payload.operation_id.to_string()) {
            return invalid_step("one operation identity binds multiple tool calls");
        }
        operations.push(proposal.into_prepared_operation(
            payload.operation_id,
            payload.implementation_version,
            payload.effect,
        ));
    }
    let bound = BoundStepOperations {
        turn_id,
        operations,
    };
    let result_facts: Vec<_> = history
        .iter()
        .filter(|event| event.schema_name.as_str() == STEP_OPERATIONS_SETTLED)
        .collect();
    if result_facts.is_empty() {
        return Ok(StepOperationProjection::Bound(bound));
    }
    if result_facts.len() != 1 {
        return invalid_step("step has multiple operation-result boundaries");
    }
    let result_event = result_facts[0];
    require_schema_v1(result_event)?;
    let expected_parent = history
        .iter()
        .rfind(|event| event.schema_name.as_str() == STEP_OPERATION_BOUND)
        .map(|event| event.event_id);
    if result_event.parent_event_id != expected_parent {
        return invalid_step("operation-result boundary does not cite the binding batch");
    }
    let payload: OperationsSettledPayload = decode_step_payload(result_event)?;
    if payload.step_id != step.step_id()
        || payload.turn_id != turn_id
        || payload.results.len() != bound.operations.len()
    {
        return invalid_step("operation-result boundary differs from bound operations");
    }
    let mut pending_results = Vec::with_capacity(payload.results.len());
    for (result, operation) in payload.results.iter().zip(&bound.operations) {
        validate_resolved_operation(events, content, result, operation)?;
        pending_results.push(result.result_id);
    }
    Ok(StepOperationProjection::Ready {
        turn_id,
        pending_results,
        operations: bound
            .operations
            .iter()
            .map(StepOperationIdentity::from_prepared)
            .collect(),
    })
}

fn validate_resolved_operation<E: EventStore, C: ContentStore>(
    events: &E,
    content: &C,
    result: &ResolvedOperation,
    operation: &PreparedToolOperation,
) -> Result<(), StepCoordinatorError> {
    if result.operation_id != operation.operation_id()
        || Some(result.tool_call_id) != operation.source_tool_call_id()
    {
        return invalid_step("resolved operation differs from its ordered binding");
    }
    let mut archived = Vec::new();
    content.write_to(&result.result_id, &mut archived)?;
    match (
        result.outcome,
        recover_tool_operation(events, result.operation_id)?,
    ) {
        (
            ResolvedOutcome::Completed,
            ToolOperationState::Completed {
                attempt_id,
                result_id,
            },
        ) if attempt_id == result.attempt_id && result_id == result.result_id => Ok(()),
        (
            ResolvedOutcome::Rejected,
            ToolOperationState::Rejected {
                attempt_id,
                diagnostic,
            },
        ) if attempt_id == result.attempt_id
            && archived
                == rejected_result_bytes(
                    result.tool_call_id,
                    result.operation_id,
                    result.attempt_id,
                    &diagnostic,
                )? =>
        {
            Ok(())
        }
        _ => invalid_step("resolved operation no longer matches its durable terminal fact"),
    }
}

fn rejected_result_bytes(
    tool_call_id: ToolCallId,
    operation_id: OperationId,
    attempt_id: AttemptId,
    diagnostic: &str,
) -> Result<Vec<u8>, StepCoordinatorError> {
    cairn_codec::to_vec(&serde_json::json!({
        "attempt_id": attempt_id.to_string(),
        "diagnostic": diagnostic,
        "operation_id": operation_id.to_string(),
        "status": "rejected",
        "tool_call_id": tool_call_id.to_string(),
    }))
    .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))
}

fn unique_event_id(
    history: &[EventEnvelope],
    schema: &str,
) -> Result<EventId, StepCoordinatorError> {
    let mut matches = history
        .iter()
        .filter(|event| event.schema_name.as_str() == schema);
    let event = matches
        .next()
        .ok_or_else(|| StepCoordinatorError::InvalidStep(format!("missing {schema} fact")))?;
    if matches.next().is_some() {
        return invalid_step(&format!("multiple {schema} facts"));
    }
    Ok(event.event_id)
}

fn require_schema_v1(event: &EventEnvelope) -> Result<(), StepCoordinatorError> {
    if event.schema_version.get() == 1 {
        Ok(())
    } else {
        invalid_step("unsupported step-operation event schema version")
    }
}

fn decode_step_payload<P: for<'de> Deserialize<'de>>(
    event: &EventEnvelope,
) -> Result<P, StepCoordinatorError> {
    cairn_codec::from_slice(&event.payload)
        .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))
}

fn new_step_fact<P: Serialize>(
    schema: &str,
    parent_event_id: EventId,
    observed_at: ObservedAtUnixMillis,
    payload: &P,
) -> Result<NewEvent, StepCoordinatorError> {
    Ok(NewEvent {
        schema_name: SchemaName::new(schema)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        schema_version: SchemaVersion::new(1)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
        parent_event_id: Some(parent_event_id),
        observed_at_unix_ms: observed_at.get(),
        payload: cairn_codec::to_vec(payload)
            .map_err(|error| StepCoordinatorError::InvalidStep(error.to_string()))?,
    })
}

fn invalid_step<T>(message: &str) -> Result<T, StepCoordinatorError> {
    Err(StepCoordinatorError::InvalidStep(message.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_protocol::{
        AttemptId, CommandId, ContentId, ContentType, ModelAttemptId, OperationId, StepId,
    };
    use cairn_record::{ContentStore, EventStore};
    use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

    use super::{
        StepOperationBlocker, StepOperationSettlement, ToolOperationAssignment, ToolRegistration,
        bind_step_operations, settle_step_operations,
    };
    use crate::{
        AdapterModelTurn, AdapterOutputItem, AdapterVersion, AgentStep, AgentStepState,
        CanonicalToolResult, ContextBlock, DeploymentName, DispatchCompletion, HistoryItem,
        InstructionBlock, ModelName, ModelSelection, OperationReconciliationEvidence,
        OperationResult, PolicyDocument, PreparedModelRequest, PreparedToolOperation, ProviderName,
        ProviderToolCallId, RecordedAdapterExchange, RecordedModelAdapter, RecordedToolExchange,
        RecordedToolGateway, ScriptedModelTransport, ScriptedToolGateway, SettledAgentStep,
        ToolCatalog, ToolEffectClass, ToolGatewayError, ToolImplementationVersion, ToolName,
        TransportError, TurnInputDecision, authorize_tool_operation, begin_model_dispatch,
        begin_tool_operation, decode_model_response, execute_model_dispatch,
        execute_tool_operation, prepare_agent_step, reconcile_tool_operation_completed,
        recover_agent_step, settle_decoded_step,
    };

    struct AwaitingFixture {
        _directory: tempfile::TempDir,
        content: SqliteContentStore,
        events: SqliteEventStore,
        step: AgentStep,
        model_attempt_id: ModelAttemptId,
    }

    fn put_json<T: ContentType>(
        content: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(value).expect("encode fixture");
        content
            .put::<T>(&mut Cursor::new(bytes))
            .expect("store fixture")
            .content_id
    }

    fn decision(content: &mut SqliteContentStore) -> TurnInputDecision {
        TurnInputDecision {
            selection: ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("fixture").expect("model"),
                deployment: DeploymentName::new("local").expect("deployment"),
                adapter_version: AdapterVersion::new("v1").expect("adapter"),
            },
            instructions: vec![put_json::<InstructionBlock>(
                content,
                &serde_json::json!({"text":"be exact"}),
            )],
            tool_catalog: put_json::<ToolCatalog>(
                content,
                &serde_json::json!({"tools":["read_source"]}),
            ),
            history: vec![put_json::<HistoryItem>(
                content,
                &serde_json::json!({"role":"user","text":"inspect"}),
            )],
            context: vec![put_json::<ContextBlock>(
                content,
                &serde_json::json!({"scope":"workspace"}),
            )],
            pending_results: Vec::new(),
            policy: put_json::<PolicyDocument>(content, &serde_json::json!({"approval":"ask"})),
        }
    }

    fn awaiting_fixture(tool_calls: usize) -> AwaitingFixture {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let step = AgentStep::new(StepId::new()).expect("step");
        let model_attempt_id = ModelAttemptId::new();
        let input = decision(&mut content);
        let authority = prepare_agent_step(
            &mut events,
            &mut content,
            &step,
            &input,
            model_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(1),
        )
        .expect("prepare step");
        let started = begin_model_dispatch(
            &mut events,
            authority,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(2),
        )
        .expect("begin model");
        let mut transport = ScriptedModelTransport::new(|_: &PreparedModelRequest| {
            Ok::<_, TransportError>(b"raw".to_vec())
        });
        let DispatchCompletion::Response(received) = execute_model_dispatch(
            &mut events,
            &mut content,
            &mut transport,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(3),
        )
        .expect("model response") else {
            panic!("expected response");
        };
        let response_id = received.response_id();
        let items = (0..tool_calls)
            .map(|index| AdapterOutputItem::ToolCall {
                provider_call_id: ProviderToolCallId::new(format!("call-{index}"))
                    .expect("provider call"),
                tool: ToolName::new("read_source").expect("tool"),
                arguments: serde_json::json!({"path":format!("src/{index}.rs")}),
            })
            .collect();
        let mut adapter = RecordedModelAdapter::new(
            AdapterVersion::new("v1").expect("adapter"),
            [RecordedAdapterExchange {
                response_id,
                turn: AdapterModelTurn { items },
            }],
        );
        let decoded = decode_model_response(
            &mut events,
            &mut content,
            &mut adapter,
            received,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(4),
        )
        .expect("decode");
        assert!(matches!(
            settle_decoded_step(
                &mut events,
                &content,
                &step,
                model_attempt_id,
                decoded,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(5),
            )
            .expect("settle semantic"),
            SettledAgentStep::AwaitingOperations { .. }
        ));
        AwaitingFixture {
            _directory: directory,
            content,
            events,
            step,
            model_attempt_id,
        }
    }

    fn registration(effect: ToolEffectClass) -> ToolRegistration {
        ToolRegistration::new(
            ToolName::new("read_source").expect("tool"),
            ToolImplementationVersion::new("v1").expect("version"),
            effect,
        )
    }

    fn complete_two_in_reverse(
        fixture: &mut AwaitingFixture,
        bound: super::BoundStepOperations,
    ) -> [ContentId<OperationResult>; 2] {
        let arguments = [
            bound.operations()[0].arguments_id(),
            bound.operations()[1].arguments_id(),
        ];
        let mut started = Vec::new();
        for operation in bound.into_operations() {
            let authority = authorize_tool_operation(
                &mut fixture.events,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(7),
                operation,
            )
            .expect("authorize operation");
            started.push(
                begin_tool_operation(
                    &mut fixture.events,
                    authority,
                    AttemptId::new(),
                    &CommandId::new(),
                    cairn_protocol::ObservedAtUnixMillis::new(8),
                )
                .expect("begin operation"),
            );
        }
        let second = started.pop().expect("second");
        let first = started.pop().expect("first");
        let mut gateway_two = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id: arguments[1],
            result: CanonicalToolResult::from_value(&serde_json::json!({"value":2}))
                .expect("result"),
        }]);
        let second_completion = execute_tool_operation(
            &mut fixture.events,
            &mut fixture.content,
            &mut gateway_two,
            second,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(9),
        )
        .expect("complete second");
        let mut gateway_one = RecordedToolGateway::new([RecordedToolExchange {
            arguments_id: arguments[0],
            result: CanonicalToolResult::from_value(&serde_json::json!({"value":1}))
                .expect("result"),
        }]);
        let first_completion = execute_tool_operation(
            &mut fixture.events,
            &mut fixture.content,
            &mut gateway_one,
            first,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(10),
        )
        .expect("complete first");
        let crate::ToolOperationCompletion::Completed {
            result_id: first_id,
            ..
        } = first_completion
        else {
            panic!("first completion");
        };
        let crate::ToolOperationCompletion::Completed {
            result_id: second_id,
            ..
        } = second_completion
        else {
            panic!("second completion");
        };
        [first_id, second_id]
    }

    #[test]
    fn bindings_are_atomic_unique_and_recover_prepared_operations() {
        let mut fixture = awaiting_fixture(2);
        let before = fixture
            .events
            .read_stream(fixture.step.stream_id(), None)
            .expect("history")
            .len();
        let wrong_registration = ToolRegistration::new(
            ToolName::new("write_source").expect("tool"),
            ToolImplementationVersion::new("v1").expect("version"),
            ToolEffectClass::ReadOnly,
        );
        assert!(
            bind_step_operations(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.step,
                fixture.model_attempt_id,
                vec![
                    ToolOperationAssignment::new(OperationId::new(), wrong_registration),
                    ToolOperationAssignment::new(
                        OperationId::new(),
                        registration(ToolEffectClass::ReadOnly),
                    ),
                ],
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(6),
            )
            .is_err()
        );
        let duplicate = OperationId::new();
        let invalid = bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            vec![
                ToolOperationAssignment::new(duplicate, registration(ToolEffectClass::ReadOnly)),
                ToolOperationAssignment::new(duplicate, registration(ToolEffectClass::ReadOnly)),
            ],
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        );
        assert!(invalid.is_err());
        assert_eq!(
            fixture
                .events
                .read_stream(fixture.step.stream_id(), None)
                .expect("history")
                .len(),
            before
        );

        let operation_ids = [OperationId::new(), OperationId::new()];
        let bound = bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            operation_ids
                .into_iter()
                .map(|id| ToolOperationAssignment::new(id, registration(ToolEffectClass::ReadOnly)))
                .collect(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("bind");
        assert_eq!(bound.operations().len(), 2);
        let AgentStepState::OperationsBound(recovered) = recover_agent_step(
            &fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
        )
        .expect("recover") else {
            panic!("expected recovered bindings");
        };
        assert_eq!(
            recovered
                .operations()
                .iter()
                .map(PreparedToolOperation::operation_id)
                .collect::<Vec<_>>(),
            operation_ids
        );
    }

    #[test]
    fn completed_results_keep_proposal_order_and_prepare_the_next_step() {
        let mut fixture = awaiting_fixture(2);
        let operation_ids = [OperationId::new(), OperationId::new()];
        let bound = bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            operation_ids
                .into_iter()
                .map(|id| ToolOperationAssignment::new(id, registration(ToolEffectClass::ReadOnly)))
                .collect(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("bind");
        let [first_id, second_id] = complete_two_in_reverse(&mut fixture, bound);
        let StepOperationSettlement::ReadyForNextStep {
            pending_results, ..
        } = settle_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(11),
        )
        .expect("settle operations")
        else {
            panic!("ready next step");
        };
        assert_eq!(pending_results, vec![first_id, second_id]);

        let mut next_decision = decision(&mut fixture.content);
        next_decision.pending_results = pending_results;
        let next_step = AgentStep::new(StepId::new()).expect("next step");
        prepare_agent_step(
            &mut fixture.events,
            &mut fixture.content,
            &next_step,
            &next_decision,
            ModelAttemptId::new(),
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(12),
        )
        .expect("pending results pass complete-input audit");
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the end-to-end fault test keeps blocker, evidence, and result flow together"
    )]
    fn ambiguous_at_most_once_blocks_until_evidence_reconciles_completion() {
        let mut fixture = awaiting_fixture(1);
        let operation_id = OperationId::new();
        let bound = bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            vec![ToolOperationAssignment::new(
                operation_id,
                registration(ToolEffectClass::AtMostOnce),
            )],
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("bind");
        let operation = bound.into_operations().pop().expect("operation");
        let authority = authorize_tool_operation(
            &mut fixture.events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
            operation,
        )
        .expect("authorize");
        let attempt_id = AttemptId::new();
        let started = begin_tool_operation(
            &mut fixture.events,
            authority,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(8),
        )
        .expect("begin");
        let mut gateway = ScriptedToolGateway::new(|_: &PreparedToolOperation| {
            Err(ToolGatewayError::Ambiguous("connection lost".into()))
        });
        execute_tool_operation(
            &mut fixture.events,
            &mut fixture.content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(9),
        )
        .expect("record ambiguity");
        let StepOperationSettlement::Blocked { blockers, .. } = settle_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(10),
        )
        .expect("classify blocker") else {
            panic!("ambiguity must block");
        };
        assert!(matches!(
            blockers.as_slice(),
            [StepOperationBlocker::ReconcileRequired {
                operation_id: actual_operation,
                attempt_id: actual_attempt,
                diagnostic: Some(_),
            }] if *actual_operation == operation_id && *actual_attempt == attempt_id
        ));
        assert!(
            !fixture
                .events
                .read_stream(fixture.step.stream_id(), None)
                .expect("history")
                .iter()
                .any(|event| event.schema_name.as_str() == "agent.step-operations-settled")
        );

        let AgentStepState::OperationsBound(recovered) = recover_agent_step(
            &fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
        )
        .expect("recover binding") else {
            panic!("binding remains recoverable");
        };
        let operation = recovered.operations()[0].clone();
        let evidence_bytes = cairn_codec::to_vec(&serde_json::json!({
            "conclusion":"completed",
            "receipt":"remote-1"
        }))
        .expect("evidence");
        let evidence_id = fixture
            .content
            .put::<OperationReconciliationEvidence>(&mut Cursor::new(evidence_bytes))
            .expect("archive evidence")
            .content_id;
        reconcile_tool_operation_completed(
            &mut fixture.events,
            &mut fixture.content,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(11),
            &operation,
            evidence_id,
            &CanonicalToolResult::from_value(&serde_json::json!({"value":1})).expect("result"),
        )
        .expect("reconcile completion");
        assert!(matches!(
            settle_step_operations(
                &mut fixture.events,
                &mut fixture.content,
                &fixture.step,
                fixture.model_attempt_id,
                &CommandId::new(),
                cairn_protocol::ObservedAtUnixMillis::new(12),
            )
            .expect("settle reconciled result"),
            StepOperationSettlement::ReadyForNextStep { .. }
        ));
    }

    #[test]
    fn definitive_rejection_becomes_verified_model_visible_feedback() {
        let mut fixture = awaiting_fixture(1);
        let operation_id = OperationId::new();
        let bound = bind_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            vec![ToolOperationAssignment::new(
                operation_id,
                registration(ToolEffectClass::ReadOnly),
            )],
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(6),
        )
        .expect("bind");
        let operation = bound.into_operations().pop().expect("operation");
        let authority = authorize_tool_operation(
            &mut fixture.events,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(7),
            operation,
        )
        .expect("authorize");
        let attempt_id = AttemptId::new();
        let started = begin_tool_operation(
            &mut fixture.events,
            authority,
            attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(8),
        )
        .expect("begin");
        let mut gateway = ScriptedToolGateway::new(|_: &PreparedToolOperation| {
            Err(ToolGatewayError::Rejected("approval denied".into()))
        });
        execute_tool_operation(
            &mut fixture.events,
            &mut fixture.content,
            &mut gateway,
            started,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(9),
        )
        .expect("record rejection");
        let StepOperationSettlement::ReadyForNextStep {
            pending_results, ..
        } = settle_step_operations(
            &mut fixture.events,
            &mut fixture.content,
            &fixture.step,
            fixture.model_attempt_id,
            &CommandId::new(),
            cairn_protocol::ObservedAtUnixMillis::new(10),
        )
        .expect("settle rejection")
        else {
            panic!("rejection is definitive feedback");
        };
        let mut bytes = Vec::new();
        fixture
            .content
            .write_to(&pending_results[0], &mut bytes)
            .expect("verified feedback");
        let feedback: serde_json::Value = cairn_codec::from_slice(&bytes).expect("feedback");
        assert_eq!(feedback["status"], "rejected");
        assert_eq!(feedback["operation_id"], operation_id.to_string());
        assert_eq!(feedback["attempt_id"], attempt_id.to_string());
        assert!(matches!(
            recover_agent_step(
                &fixture.events,
                &mut fixture.content,
                &fixture.step,
                fixture.model_attempt_id,
            )
            .expect("recover ready"),
            AgentStepState::ReadyForNextStep { .. }
        ));
    }
}
