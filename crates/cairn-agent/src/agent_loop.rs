//! Role-scoped Agent Loop orchestration above the durable model/tool episode driver.

use std::{collections::BTreeMap, future::Future};

use cairn_protocol::{AgentLoopId, ContentId, TaskId};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{
    AgentHookProfileName, AgentHookProfileVersion, AgentLoopContextArtifact,
    AgentLoopSuspensionReason, AgentRoleName, KnowledgeIndexArtifact, KnowledgeSnapshotVersion,
    KnowledgeSourceName, SkillDefinitionArtifact, SkillImplementationVersion, SkillName,
    ToolDescriptorArtifact, ToolName, ToolRegistration,
};

/// Positive upper bound on steps started by one role-scoped Agent Loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct AgentLoopStepLimit(u32);

impl AgentLoopStepLimit {
    /// Creates a positive step limit.
    ///
    /// # Errors
    ///
    /// Returns an error for zero because an initialized loop must be able to start a step.
    pub const fn new(value: u32) -> Result<Self, AgentLoopRegistryError> {
        if value == 0 {
            Err(AgentLoopRegistryError::ZeroStepLimit)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the configured bound.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for AgentLoopStepLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

/// Immutable identity and policy binding used to initialize an Agent Loop from a product role.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLoopStartV1 {
    loop_id: AgentLoopId,
    task_id: TaskId,
    role: AgentRoleName,
    hook_profile: AgentHookProfileName,
    hook_version: AgentHookProfileVersion,
    context: ContentId<AgentLoopContextArtifact>,
    step_limit: AgentLoopStepLimit,
}

impl AgentLoopStartV1 {
    /// Freezes the role, context, hook implementation, and budget before any model step starts.
    #[must_use]
    pub const fn new(
        loop_id: AgentLoopId,
        task_id: TaskId,
        role: AgentRoleName,
        hook_profile: AgentHookProfileName,
        hook_version: AgentHookProfileVersion,
        context: ContentId<AgentLoopContextArtifact>,
        step_limit: AgentLoopStepLimit,
    ) -> Self {
        Self {
            loop_id,
            task_id,
            role,
            hook_profile,
            hook_version,
            context,
            step_limit,
        }
    }

    #[must_use]
    pub const fn loop_id(&self) -> AgentLoopId {
        self.loop_id
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn role(&self) -> &AgentRoleName {
        &self.role
    }

    #[must_use]
    pub const fn hook_profile(&self) -> &AgentHookProfileName {
        &self.hook_profile
    }

    #[must_use]
    pub const fn hook_version(&self) -> &AgentHookProfileVersion {
        &self.hook_version
    }

    #[must_use]
    pub const fn context(&self) -> ContentId<AgentLoopContextArtifact> {
        self.context
    }
}

/// Persistable loop position. It never contains a model response or product artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(try_from = "AgentLoopCheckpointWireV1")]
pub struct AgentLoopCheckpointV1 {
    start: AgentLoopStartV1,
    steps_started: u32,
    status: AgentLoopStatusV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentLoopCheckpointWireV1 {
    start: AgentLoopStartV1,
    steps_started: u32,
    status: AgentLoopStatusV1,
}

impl TryFrom<AgentLoopCheckpointWireV1> for AgentLoopCheckpointV1 {
    type Error = AgentLoopRegistryError;

    fn try_from(wire: AgentLoopCheckpointWireV1) -> Result<Self, Self::Error> {
        if wire.steps_started > wire.start.step_limit.get() {
            return Err(AgentLoopRegistryError::InvalidCheckpoint);
        }
        if wire.steps_started == 0
            && matches!(
                wire.status,
                AgentLoopStatusV1::Suspended(_) | AgentLoopStatusV1::Complete
            )
        {
            return Err(AgentLoopRegistryError::InvalidCheckpoint);
        }
        Ok(Self {
            start: wire.start,
            steps_started: wire.steps_started,
            status: wire.status,
        })
    }
}

impl<'de> Deserialize<'de> for AgentLoopCheckpointV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        AgentLoopCheckpointWireV1::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

impl AgentLoopCheckpointV1 {
    #[must_use]
    pub const fn start(&self) -> &AgentLoopStartV1 {
        &self.start
    }

    #[must_use]
    pub const fn steps_started(&self) -> u32 {
        self.steps_started
    }

    #[must_use]
    pub const fn status(&self) -> &AgentLoopStatusV1 {
        &self.status
    }
}

/// Durable control position of a role-scoped loop.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentLoopStatusV1 {
    Ready,
    Suspended(AgentLoopSuspensionReason),
    Complete,
}

/// Tool metadata visible to the model. It is not invocation authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolIndexEntryV1 {
    name: ToolName,
    descriptor: ContentId<ToolDescriptorArtifact>,
}

impl ToolIndexEntryV1 {
    #[must_use]
    pub const fn name(&self) -> &ToolName {
        &self.name
    }

    #[must_use]
    pub const fn descriptor(&self) -> ContentId<ToolDescriptorArtifact> {
        self.descriptor
    }
}

/// Exact tool index selected by a context-exposure hook for one step.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolIndexViewV1(Vec<ToolIndexEntryV1>);

impl ToolIndexViewV1 {
    #[must_use]
    pub fn entries(&self) -> &[ToolIndexEntryV1] {
        &self.0
    }
}

#[derive(Clone, Debug)]
struct RegisteredToolV1 {
    descriptor: ContentId<ToolDescriptorArtifact>,
    execution: ToolRegistration,
}

/// Process-local registry of available tool implementations.
#[derive(Debug, Default)]
pub struct ToolRegistry(BTreeMap<ToolName, RegisteredToolV1>);

impl ToolRegistry {
    /// Registers one exact trusted implementation and its separately archived descriptor.
    ///
    /// # Errors
    ///
    /// Rejects a tool name that is already registered.
    pub fn register(
        &mut self,
        execution: ToolRegistration,
        descriptor: ContentId<ToolDescriptorArtifact>,
    ) -> Result<(), AgentLoopRegistryError> {
        let name = execution.name().clone();
        if self.0.contains_key(&name) {
            return Err(AgentLoopRegistryError::DuplicateTool(name));
        }
        self.0.insert(
            name,
            RegisteredToolV1 {
                descriptor,
                execution,
            },
        );
        Ok(())
    }

    /// Selects the exact tool metadata visible to one step without granting invocation.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated tool names.
    pub fn index(&self, visible: &[ToolName]) -> Result<ToolIndexViewV1, AgentLoopRegistryError> {
        reject_repeated(visible, AgentLoopRegistryError::RepeatedToolExposure)?;
        visible
            .iter()
            .map(|name| {
                self.0
                    .get(name)
                    .map(|registered| ToolIndexEntryV1 {
                        name: name.clone(),
                        descriptor: registered.descriptor,
                    })
                    .ok_or_else(|| AgentLoopRegistryError::UnknownTool(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(ToolIndexViewV1)
    }

    /// Selects exact trusted implementations authorized for one step.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated tool names.
    pub fn grant(
        &self,
        authorized: &[ToolName],
    ) -> Result<ToolInvocationGrantV1, AgentLoopRegistryError> {
        reject_repeated(authorized, AgentLoopRegistryError::RepeatedToolAuthority)?;
        authorized
            .iter()
            .map(|name| {
                self.0
                    .get(name)
                    .map(|registered| registered.execution.clone())
                    .ok_or_else(|| AgentLoopRegistryError::UnknownTool(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(ToolInvocationGrantV1)
    }
}

/// Trusted tool implementations authorized for invocation during one step.
#[derive(Debug)]
pub struct ToolInvocationGrantV1(Vec<ToolRegistration>);

impl ToolInvocationGrantV1 {
    #[must_use]
    pub fn registrations(&self) -> &[ToolRegistration] {
        &self.0
    }
}

/// Registered skill package. Discovery and activation remain separate decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillRegistrationV1 {
    name: SkillName,
    version: SkillImplementationVersion,
    definition: ContentId<SkillDefinitionArtifact>,
}

impl SkillRegistrationV1 {
    #[must_use]
    pub const fn new(
        name: SkillName,
        version: SkillImplementationVersion,
        definition: ContentId<SkillDefinitionArtifact>,
    ) -> Self {
        Self {
            name,
            version,
            definition,
        }
    }

    #[must_use]
    pub const fn name(&self) -> &SkillName {
        &self.name
    }

    #[must_use]
    pub const fn version(&self) -> &SkillImplementationVersion {
        &self.version
    }

    #[must_use]
    pub const fn definition(&self) -> ContentId<SkillDefinitionArtifact> {
        self.definition
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillIndexEntryV1 {
    name: SkillName,
    definition: ContentId<SkillDefinitionArtifact>,
}

impl SkillIndexEntryV1 {
    #[must_use]
    pub const fn name(&self) -> &SkillName {
        &self.name
    }

    #[must_use]
    pub const fn definition(&self) -> ContentId<SkillDefinitionArtifact> {
        self.definition
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SkillIndexViewV1(Vec<SkillIndexEntryV1>);

impl SkillIndexViewV1 {
    #[must_use]
    pub fn entries(&self) -> &[SkillIndexEntryV1] {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct SkillRegistry(BTreeMap<SkillName, SkillRegistrationV1>);

impl SkillRegistry {
    /// Registers one exact skill package.
    ///
    /// # Errors
    ///
    /// Rejects a skill name that is already registered.
    pub fn register(
        &mut self,
        registration: SkillRegistrationV1,
    ) -> Result<(), AgentLoopRegistryError> {
        let name = registration.name.clone();
        if self.0.contains_key(&name) {
            return Err(AgentLoopRegistryError::DuplicateSkill(name));
        }
        self.0.insert(name, registration);
        Ok(())
    }

    /// Selects skill metadata visible to one step without granting activation.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated skill names.
    pub fn index(&self, visible: &[SkillName]) -> Result<SkillIndexViewV1, AgentLoopRegistryError> {
        reject_repeated(visible, AgentLoopRegistryError::RepeatedSkillExposure)?;
        visible
            .iter()
            .map(|name| {
                self.0
                    .get(name)
                    .map(|registered| SkillIndexEntryV1 {
                        name: name.clone(),
                        definition: registered.definition,
                    })
                    .ok_or_else(|| AgentLoopRegistryError::UnknownSkill(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(SkillIndexViewV1)
    }

    /// Selects exact skill packages authorized for activation during one step.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated skill names.
    pub fn grant(
        &self,
        authorized: &[SkillName],
    ) -> Result<SkillActivationGrantV1, AgentLoopRegistryError> {
        reject_repeated(authorized, AgentLoopRegistryError::RepeatedSkillAuthority)?;
        authorized
            .iter()
            .map(|name| {
                self.0
                    .get(name)
                    .cloned()
                    .ok_or_else(|| AgentLoopRegistryError::UnknownSkill(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(SkillActivationGrantV1)
    }
}

#[derive(Debug)]
pub struct SkillActivationGrantV1(Vec<SkillRegistrationV1>);

impl SkillActivationGrantV1 {
    #[must_use]
    pub fn registrations(&self) -> &[SkillRegistrationV1] {
        &self.0
    }
}

/// Registered immutable knowledge snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnowledgeRegistrationV1 {
    source: KnowledgeSourceName,
    snapshot_version: KnowledgeSnapshotVersion,
    index: ContentId<KnowledgeIndexArtifact>,
}

impl KnowledgeRegistrationV1 {
    #[must_use]
    pub const fn new(
        source: KnowledgeSourceName,
        snapshot_version: KnowledgeSnapshotVersion,
        index: ContentId<KnowledgeIndexArtifact>,
    ) -> Self {
        Self {
            source,
            snapshot_version,
            index,
        }
    }

    #[must_use]
    pub const fn source(&self) -> &KnowledgeSourceName {
        &self.source
    }

    #[must_use]
    pub const fn snapshot_version(&self) -> &KnowledgeSnapshotVersion {
        &self.snapshot_version
    }

    #[must_use]
    pub const fn index(&self) -> ContentId<KnowledgeIndexArtifact> {
        self.index
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KnowledgeIndexEntryV1 {
    source: KnowledgeSourceName,
    index: ContentId<KnowledgeIndexArtifact>,
}

impl KnowledgeIndexEntryV1 {
    #[must_use]
    pub const fn source(&self) -> &KnowledgeSourceName {
        &self.source
    }

    #[must_use]
    pub const fn index(&self) -> ContentId<KnowledgeIndexArtifact> {
        self.index
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KnowledgeIndexViewV1(Vec<KnowledgeIndexEntryV1>);

impl KnowledgeIndexViewV1 {
    #[must_use]
    pub fn entries(&self) -> &[KnowledgeIndexEntryV1] {
        &self.0
    }
}

#[derive(Debug, Default)]
pub struct KnowledgeRegistry(BTreeMap<KnowledgeSourceName, KnowledgeRegistrationV1>);

impl KnowledgeRegistry {
    /// Registers one immutable knowledge snapshot.
    ///
    /// # Errors
    ///
    /// Rejects a knowledge source that is already registered.
    pub fn register(
        &mut self,
        registration: KnowledgeRegistrationV1,
    ) -> Result<(), AgentLoopRegistryError> {
        let source = registration.source.clone();
        if self.0.contains_key(&source) {
            return Err(AgentLoopRegistryError::DuplicateKnowledge(source));
        }
        self.0.insert(source, registration);
        Ok(())
    }

    /// Selects knowledge indexes visible to one step without granting reads.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated knowledge sources.
    pub fn index(
        &self,
        visible: &[KnowledgeSourceName],
    ) -> Result<KnowledgeIndexViewV1, AgentLoopRegistryError> {
        reject_repeated(visible, AgentLoopRegistryError::RepeatedKnowledgeExposure)?;
        visible
            .iter()
            .map(|source| {
                self.0
                    .get(source)
                    .map(|registered| KnowledgeIndexEntryV1 {
                        source: source.clone(),
                        index: registered.index,
                    })
                    .ok_or_else(|| AgentLoopRegistryError::UnknownKnowledge(source.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(KnowledgeIndexViewV1)
    }

    /// Selects exact knowledge snapshots authorized for reads during one step.
    ///
    /// # Errors
    ///
    /// Rejects unknown or repeated knowledge sources.
    pub fn grant(
        &self,
        authorized: &[KnowledgeSourceName],
    ) -> Result<KnowledgeReadGrantV1, AgentLoopRegistryError> {
        reject_repeated(
            authorized,
            AgentLoopRegistryError::RepeatedKnowledgeAuthority,
        )?;
        authorized
            .iter()
            .map(|source| {
                self.0
                    .get(source)
                    .cloned()
                    .ok_or_else(|| AgentLoopRegistryError::UnknownKnowledge(source.clone()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(KnowledgeReadGrantV1)
    }
}

#[derive(Debug)]
pub struct KnowledgeReadGrantV1(Vec<KnowledgeRegistrationV1>);

impl KnowledgeReadGrantV1 {
    #[must_use]
    pub fn registrations(&self) -> &[KnowledgeRegistrationV1] {
        &self.0
    }
}

/// All process-local registries available to a product hook. Merely registering an item exposes
/// nothing to a model and grants no effect authority.
#[derive(Clone, Copy)]
pub struct AgentRegistries<'a> {
    pub tools: &'a ToolRegistry,
    pub skills: &'a SkillRegistry,
    pub knowledge: &'a KnowledgeRegistry,
}

/// Product context projected into an Agent Loop. The content identity binds the in-memory value to
/// the exact snapshot frozen in [`AgentLoopStartV1`].
pub trait AgentLoopContext: Sync {
    fn context_id(&self) -> ContentId<AgentLoopContextArtifact>;
}

/// Model-visible registry indexes selected for one exact step.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentContextExposureV1 {
    pub tools: ToolIndexViewV1,
    pub skills: SkillIndexViewV1,
    pub knowledge: KnowledgeIndexViewV1,
}

/// Exact step access selected by hooks. Discovery views and effect authorities cannot be confused.
#[derive(Debug)]
pub struct AgentStepAccessV1 {
    pub exposure: AgentContextExposureV1,
    pub tool_invocation: ToolInvocationGrantV1,
    pub skill_activation: SkillActivationGrantV1,
    pub knowledge_read: KnowledgeReadGrantV1,
}

/// Role-specific hook decisions frozen at loop initialization.
pub trait AgentLoopHooks<C>: Send + Sync {
    type StepObservation: Send;
    type Output: Send;
    type Error: Send;

    fn profile(&self) -> (&AgentHookProfileName, &AgentHookProfileVersion);

    /// Builds the first exact step access decision from the frozen role context.
    ///
    /// # Errors
    ///
    /// Returns the product hook error when context projection or access policy fails.
    fn initialize(
        &self,
        start: &AgentLoopStartV1,
        context: &C,
        registries: AgentRegistries<'_>,
    ) -> Result<AgentLoopInitializationV1, Self::Error>;

    /// Re-evaluates exact model visibility and authority before the next step.
    ///
    /// # Errors
    ///
    /// Returns the product hook error when context projection or access policy fails.
    fn before_step(
        &self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &C,
        registries: AgentRegistries<'_>,
    ) -> Result<AgentStepAccessV1, Self::Error>;

    /// Interprets a step observation as continue, suspend, or complete for this role.
    ///
    /// # Errors
    ///
    /// Returns the product hook error when the observation violates role policy.
    fn after_step(
        &self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &C,
        observation: Self::StepObservation,
    ) -> Result<AgentLoopDirectiveV1<Self::Output>, Self::Error>;
}

/// Product hook result produced before the first step receives authority.
#[derive(Debug)]
pub struct AgentLoopInitializationV1 {
    pub first_step_access: AgentStepAccessV1,
}

/// A lower-level durable executor runs one model/tool step and returns at an external-effect yield.
pub trait AgentLoopStepExecutor<C, O>: Send {
    type Error: Send;

    fn execute_step(
        &mut self,
        checkpoint: &AgentLoopCheckpointV1,
        context: &C,
        access: &AgentStepAccessV1,
    ) -> impl Future<Output = Result<AgentLoopStepExecutionV1<O>, Self::Error>> + Send;
}

/// Boundary returned by one durable step execution.
pub enum AgentLoopStepExecutionV1<O> {
    Observed(O),
    Suspended(AgentLoopSuspensionReason),
}

/// Role hook decision after observing one completed step.
pub enum AgentLoopDirectiveV1<T> {
    Continue,
    Suspend(AgentLoopSuspensionReason),
    Complete(T),
}

/// Initialized, not-yet-run loop and its first exact access decision.
pub struct InitializedAgentLoopV1 {
    checkpoint: AgentLoopCheckpointV1,
    next_access: AgentStepAccessV1,
}

/// Checks role/hook binding and initializes a first-class Agent Loop.
///
/// # Errors
///
/// Rejects context or hook binding drift and propagates product hook initialization failures.
pub fn initialize_agent_loop<C, H>(
    start: AgentLoopStartV1,
    context: &C,
    hooks: &H,
    registries: AgentRegistries<'_>,
) -> Result<InitializedAgentLoopV1, AgentLoopRunError<H::Error, std::convert::Infallible>>
where
    C: AgentLoopContext,
    H: AgentLoopHooks<C>,
{
    if context.context_id() != start.context() {
        return Err(AgentLoopRunError::ContextBindingMismatch);
    }
    let (profile, version) = hooks.profile();
    if profile != start.hook_profile() || version != start.hook_version() {
        return Err(AgentLoopRunError::HookBindingMismatch);
    }
    let initialization = hooks
        .initialize(&start, context, registries)
        .map_err(AgentLoopRunError::Hook)?;
    tracing::info!(
        target: "cairn.agent.loop",
        event = "agent_loop_initialized",
        loop_id = %start.loop_id(),
        task_id = %start.task_id(),
        role = %start.role(),
        hook_profile = %start.hook_profile(),
        hook_version = %start.hook_version(),
        "role-scoped Agent Loop initialized"
    );
    Ok(InitializedAgentLoopV1 {
        checkpoint: AgentLoopCheckpointV1 {
            start,
            steps_started: 0,
            status: AgentLoopStatusV1::Ready,
        },
        next_access: initialization.first_step_access,
    })
}

/// Rehydrates a suspended checkpoint and asks the current role hooks for the next exact access
/// decision. Hook identity/version must still match the frozen start request.
///
/// # Errors
///
/// Rejects a non-suspended checkpoint, context or hook binding drift, and hook policy failures.
pub fn resume_agent_loop<C, H>(
    mut checkpoint: AgentLoopCheckpointV1,
    context: &C,
    hooks: &H,
    registries: AgentRegistries<'_>,
) -> Result<InitializedAgentLoopV1, AgentLoopRunError<H::Error, std::convert::Infallible>>
where
    C: AgentLoopContext,
    H: AgentLoopHooks<C>,
{
    if !matches!(checkpoint.status, AgentLoopStatusV1::Suspended(_)) {
        return Err(AgentLoopRunError::CheckpointNotSuspended);
    }
    let (profile, version) = hooks.profile();
    if profile != checkpoint.start.hook_profile() || version != checkpoint.start.hook_version() {
        return Err(AgentLoopRunError::HookBindingMismatch);
    }
    if context.context_id() != checkpoint.start.context() {
        return Err(AgentLoopRunError::ContextBindingMismatch);
    }
    checkpoint.status = AgentLoopStatusV1::Ready;
    let next_access = hooks
        .before_step(&checkpoint, context, registries)
        .map_err(AgentLoopRunError::Hook)?;
    tracing::info!(
        target: "cairn.agent.loop",
        event = "agent_loop_resumed",
        loop_id = %checkpoint.start.loop_id(),
        task_id = %checkpoint.start.task_id(),
        role = %checkpoint.start.role(),
        steps_started = checkpoint.steps_started,
        "Agent Loop resumed from durable checkpoint"
    );
    Ok(InitializedAgentLoopV1 {
        checkpoint,
        next_access,
    })
}

/// Runs a role-scoped Agent Loop through hook-selected steps until completion or a durable yield.
pub async fn run_agent_loop<C, H, E>(
    mut initialized: InitializedAgentLoopV1,
    context: &C,
    hooks: &H,
    registries: AgentRegistries<'_>,
    executor: &mut E,
) -> Result<AgentLoopRunOutcomeV1<H::Output>, AgentLoopRunError<H::Error, E::Error>>
where
    C: AgentLoopContext,
    H: AgentLoopHooks<C>,
    E: AgentLoopStepExecutor<C, H::StepObservation>,
{
    if context.context_id() != initialized.checkpoint.start.context() {
        return Err(AgentLoopRunError::ContextBindingMismatch);
    }
    loop {
        if initialized.checkpoint.steps_started == initialized.checkpoint.start.step_limit.get() {
            return Err(AgentLoopRunError::StepLimitReached);
        }
        initialized.checkpoint.steps_started += 1;
        tracing::info!(
            target: "cairn.agent.loop",
            event = "agent_loop_step_started",
            loop_id = %initialized.checkpoint.start.loop_id(),
            task_id = %initialized.checkpoint.start.task_id(),
            role = %initialized.checkpoint.start.role(),
            step_ordinal = initialized.checkpoint.steps_started,
            "Agent Loop step started"
        );
        let execution = executor
            .execute_step(&initialized.checkpoint, context, &initialized.next_access)
            .await
            .map_err(AgentLoopRunError::Executor)?;
        let AgentLoopStepExecutionV1::Observed(observation) = execution else {
            let AgentLoopStepExecutionV1::Suspended(reason) = execution else {
                unreachable!()
            };
            initialized.checkpoint.status = AgentLoopStatusV1::Suspended(reason.clone());
            tracing::info!(
                target: "cairn.agent.loop",
                event = "agent_loop_suspended",
                loop_id = %initialized.checkpoint.start.loop_id(),
                task_id = %initialized.checkpoint.start.task_id(),
                role = %initialized.checkpoint.start.role(),
                "Agent Loop yielded at an external boundary"
            );
            return Ok(AgentLoopRunOutcomeV1::Suspended(initialized.checkpoint));
        };
        match hooks
            .after_step(&initialized.checkpoint, context, observation)
            .map_err(AgentLoopRunError::Hook)?
        {
            AgentLoopDirectiveV1::Continue => {
                initialized.next_access = hooks
                    .before_step(&initialized.checkpoint, context, registries)
                    .map_err(AgentLoopRunError::Hook)?;
            }
            AgentLoopDirectiveV1::Suspend(reason) => {
                initialized.checkpoint.status = AgentLoopStatusV1::Suspended(reason);
                return Ok(AgentLoopRunOutcomeV1::Suspended(initialized.checkpoint));
            }
            AgentLoopDirectiveV1::Complete(output) => {
                initialized.checkpoint.status = AgentLoopStatusV1::Complete;
                tracing::info!(
                    target: "cairn.agent.loop",
                    event = "agent_loop_completed",
                    loop_id = %initialized.checkpoint.start.loop_id(),
                    task_id = %initialized.checkpoint.start.task_id(),
                    role = %initialized.checkpoint.start.role(),
                    steps_started = initialized.checkpoint.steps_started,
                    "Agent Loop completed"
                );
                return Ok(AgentLoopRunOutcomeV1::Complete {
                    checkpoint: initialized.checkpoint,
                    output,
                });
            }
        }
    }
}

pub enum AgentLoopRunOutcomeV1<T> {
    Suspended(AgentLoopCheckpointV1),
    Complete {
        checkpoint: AgentLoopCheckpointV1,
        output: T,
    },
}

#[derive(Debug, Error)]
pub enum AgentLoopRunError<H, E> {
    #[error("Agent Loop hook binding does not match the frozen start request")]
    HookBindingMismatch,
    #[error("Agent Loop context does not match the frozen context identity")]
    ContextBindingMismatch,
    #[error("only a suspended Agent Loop checkpoint can be resumed")]
    CheckpointNotSuspended,
    #[error("Agent Loop reached its step limit before the role completed")]
    StepLimitReached,
    #[error("Agent Loop hook failed")]
    Hook(H),
    #[error("Agent Loop step executor failed")]
    Executor(E),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum AgentLoopRegistryError {
    #[error("Agent Loop step limit must be greater than zero")]
    ZeroStepLimit,
    #[error("Agent Loop checkpoint violates its frozen bounds")]
    InvalidCheckpoint,
    #[error("tool is already registered: {0}")]
    DuplicateTool(ToolName),
    #[error("tool is not registered: {0}")]
    UnknownTool(ToolName),
    #[error("skill is already registered: {0}")]
    DuplicateSkill(SkillName),
    #[error("skill is not registered: {0}")]
    UnknownSkill(SkillName),
    #[error("knowledge source is already registered: {0}")]
    DuplicateKnowledge(KnowledgeSourceName),
    #[error("knowledge source is not registered: {0}")]
    UnknownKnowledge(KnowledgeSourceName),
    #[error("tool exposure contains a duplicate entry")]
    RepeatedToolExposure,
    #[error("tool invocation authority contains a duplicate entry")]
    RepeatedToolAuthority,
    #[error("skill exposure contains a duplicate entry")]
    RepeatedSkillExposure,
    #[error("skill activation authority contains a duplicate entry")]
    RepeatedSkillAuthority,
    #[error("knowledge exposure contains a duplicate entry")]
    RepeatedKnowledgeExposure,
    #[error("knowledge read authority contains a duplicate entry")]
    RepeatedKnowledgeAuthority,
}

fn reject_repeated<T: Ord>(
    values: &[T],
    error: AgentLoopRegistryError,
) -> Result<(), AgentLoopRegistryError> {
    let mut seen = std::collections::BTreeSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;
    use crate::{ToolEffectClass, ToolImplementationVersion};

    #[test]
    fn discovery_does_not_imply_authority() {
        let name = ToolName::new("inspect-source").expect("tool name");
        let descriptor = ContentId::<ToolDescriptorArtifact>::derive(b"descriptor")
            .expect("descriptor identity");
        let mut tools = ToolRegistry::default();
        tools
            .register(
                ToolRegistration::new(
                    name.clone(),
                    ToolImplementationVersion::new("builtin-v1").expect("version"),
                    ToolEffectClass::ReadOnly,
                ),
                descriptor,
            )
            .expect("registration");

        let visible = tools.index(std::slice::from_ref(&name)).expect("index");
        let authority = tools.grant(&[]).expect("empty authority");

        assert_eq!(visible.entries()[0].name(), &name);
        assert!(authority.registrations().is_empty());
    }

    #[test]
    fn duplicate_registration_does_not_replace_original_authority() {
        let name = ToolName::new("inspect-source").expect("tool name");
        let first_descriptor = ContentId::<ToolDescriptorArtifact>::derive(b"first")
            .expect("first descriptor identity");
        let second_descriptor = ContentId::<ToolDescriptorArtifact>::derive(b"second")
            .expect("second descriptor identity");
        let mut tools = ToolRegistry::default();
        tools
            .register(
                ToolRegistration::new(
                    name.clone(),
                    ToolImplementationVersion::new("builtin-v1").expect("version"),
                    ToolEffectClass::ReadOnly,
                ),
                first_descriptor,
            )
            .expect("first registration");

        assert_eq!(
            tools.register(
                ToolRegistration::new(
                    name.clone(),
                    ToolImplementationVersion::new("builtin-v2").expect("version"),
                    ToolEffectClass::AmbiguousExternal,
                ),
                second_descriptor,
            ),
            Err(AgentLoopRegistryError::DuplicateTool(name.clone()))
        );
        assert_eq!(
            tools
                .index(std::slice::from_ref(&name))
                .expect("preserved index")
                .entries()[0]
                .descriptor(),
            first_descriptor
        );
        assert_eq!(
            tools
                .grant(&[name])
                .expect("preserved grant")
                .registrations()[0]
                .effect(),
            ToolEffectClass::ReadOnly
        );
    }

    #[test]
    fn checkpoint_deserialization_rechecks_frozen_step_bound() {
        let start = AgentLoopStartV1::new(
            AgentLoopId::new(),
            TaskId::new(),
            AgentRoleName::new("reviewer").expect("role"),
            AgentHookProfileName::new("review-hooks").expect("profile"),
            AgentHookProfileVersion::new("builtin-v1").expect("version"),
            ContentId::<AgentLoopContextArtifact>::derive(b"context").expect("context"),
            AgentLoopStepLimit::new(1).expect("limit"),
        );
        let checkpoint = AgentLoopCheckpointV1 {
            start,
            steps_started: 1,
            status: AgentLoopStatusV1::Ready,
        };
        let mut wire = serde_json::to_value(checkpoint).expect("serialize");
        wire["steps_started"] = serde_json::json!(2);

        assert!(serde_json::from_value::<AgentLoopCheckpointV1>(wire).is_err());
    }

    struct Context(ContentId<AgentLoopContextArtifact>);

    impl AgentLoopContext for Context {
        fn context_id(&self) -> ContentId<AgentLoopContextArtifact> {
            self.0
        }
    }

    struct Hooks {
        profile: AgentHookProfileName,
        version: AgentHookProfileVersion,
    }

    impl AgentLoopHooks<Context> for Hooks {
        type StepObservation = ();
        type Output = ();
        type Error = Infallible;

        fn profile(&self) -> (&AgentHookProfileName, &AgentHookProfileVersion) {
            (&self.profile, &self.version)
        }

        fn initialize(
            &self,
            _start: &AgentLoopStartV1,
            _context: &Context,
            _registries: AgentRegistries<'_>,
        ) -> Result<AgentLoopInitializationV1, Infallible> {
            unreachable!("context mismatch must fail before hooks run")
        }

        fn before_step(
            &self,
            _checkpoint: &AgentLoopCheckpointV1,
            _context: &Context,
            _registries: AgentRegistries<'_>,
        ) -> Result<AgentStepAccessV1, Infallible> {
            unreachable!("not used")
        }

        fn after_step(
            &self,
            _checkpoint: &AgentLoopCheckpointV1,
            _context: &Context,
            _observation: (),
        ) -> Result<AgentLoopDirectiveV1<()>, Infallible> {
            unreachable!("not used")
        }
    }

    #[test]
    fn initialization_rejects_context_lineage_substitution() {
        let expected = ContentId::<AgentLoopContextArtifact>::derive(b"expected").expect("context");
        let substituted =
            ContentId::<AgentLoopContextArtifact>::derive(b"substituted").expect("context");
        let profile = AgentHookProfileName::new("review-hooks").expect("profile");
        let version = AgentHookProfileVersion::new("builtin-v1").expect("version");
        let start = AgentLoopStartV1::new(
            AgentLoopId::new(),
            TaskId::new(),
            AgentRoleName::new("reviewer").expect("role"),
            profile.clone(),
            version.clone(),
            expected,
            AgentLoopStepLimit::new(1).expect("limit"),
        );
        let tools = ToolRegistry::default();
        let skills = SkillRegistry::default();
        let knowledge = KnowledgeRegistry::default();

        assert!(matches!(
            initialize_agent_loop(
                start,
                &Context(substituted),
                &Hooks { profile, version },
                AgentRegistries {
                    tools: &tools,
                    skills: &skills,
                    knowledge: &knowledge,
                },
            ),
            Err(AgentLoopRunError::ContextBindingMismatch)
        ));
    }
}
