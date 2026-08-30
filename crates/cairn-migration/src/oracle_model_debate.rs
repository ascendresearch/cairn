//! Optional model-backed Oracle debate plan and cache-stable strategy projections.

use std::{collections::HashSet, io::Cursor};

use cairn_agent::{
    ContextBlock, EpisodeBudget, InstructionBlock, ResolvedRuntimeModelArtifact, ToolCatalog,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use cairn_record::{ContentStore, ContentStoreError};
use cairn_verification::{
    AdmissionPolicyArtifact, DeclaredDomainArtifact, ModelConfigurationArtifact,
    OracleTaskInputArtifact,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_V1: u16 = 1;

/// Durable semantic domain for one complete `OracleModelDebate` plan.
pub enum OracleModelDebatePlanArtifact {}

impl ContentType for OracleModelDebatePlanArtifact {
    const DOMAIN: &'static str = "migration.oracle-model-debate-plan.v1";
}

/// Closed strategy profile used only inside the optional model-backed debate implementation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleDebateStrategy {
    /// Domain/reference/property/corpus author.
    Synthesis,
    /// False-accept and false-reject breaker.
    Adversarial,
}

/// Product tool capability offered to one model-backed debate strategy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleDebateTool {
    /// Search and fetch bounded external/upstream test evidence.
    SearchExternalTests,
    /// Submit an evidence-citing domain refinement.
    SubmitDomainRefinement,
    /// Submit a complete oracle proposal revision.
    SubmitOracleProposal,
    /// Submit a correct-by-construction admission variant.
    SubmitCorrectVariant,
    /// Submit a deliberately wrong admission variant.
    SubmitWrongVariant,
    /// Submit an adversarial corpus proposal.
    SubmitAdversarialCase,
}

impl OracleDebateStrategy {
    const fn required_tools(self) -> &'static [OracleDebateTool] {
        match self {
            Self::Synthesis => &[
                OracleDebateTool::SearchExternalTests,
                OracleDebateTool::SubmitDomainRefinement,
                OracleDebateTool::SubmitOracleProposal,
            ],
            Self::Adversarial => &[
                OracleDebateTool::SubmitCorrectVariant,
                OracleDebateTool::SubmitWrongVariant,
                OracleDebateTool::SubmitAdversarialCase,
            ],
        }
    }
}

/// Constructor input for one strategy-bound generic agent episode.
pub struct OracleDebateEpisodeInput {
    /// Closed model-backed debate strategy.
    pub strategy: OracleDebateStrategy,
    /// Distinct durable generic agent episode.
    pub episode_id: EpisodeId,
    /// Frozen resolved model configuration used by the episode.
    pub model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    /// Verification-domain identity of the same frozen model configuration for authorship edges.
    pub authorship_configuration: ContentId<ModelConfigurationArtifact>,
    /// One stable strategy-specific instruction block after common instructions.
    pub strategy_instruction: ContentId<InstructionBlock>,
    /// Optional strategy-private submitted context roots in append order.
    pub private_context: Vec<ContentId<ContextBlock>>,
    /// Independently enforced durable budget for this strategy episode.
    pub budget: EpisodeBudget,
}

/// Exact strategy, model, prompt, tool, and private-context binding for one episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleDebateEpisodeWire", into = "OracleDebateEpisodeWire")]
pub struct OracleDebateEpisodeV1 {
    schema_version: u16,
    strategy: OracleDebateStrategy,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    authorship_configuration: ContentId<ModelConfigurationArtifact>,
    strategy_instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    tools: Vec<OracleDebateTool>,
    private_context: Vec<ContentId<ContextBlock>>,
    budget: EpisodeBudget,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleDebateEpisodeWire {
    schema_version: u16,
    strategy: OracleDebateStrategy,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    authorship_configuration: ContentId<ModelConfigurationArtifact>,
    strategy_instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    tools: Vec<OracleDebateTool>,
    private_context: Vec<ContentId<ContextBlock>>,
    budget: EpisodeBudget,
}

impl OracleDebateEpisodeV1 {
    fn new(input: OracleDebateEpisodeInput) -> Result<Self, OracleModelDebatePlanError> {
        validate_unique(&input.private_context, "strategy private context")?;
        let tools = input.strategy.required_tools().to_vec();
        Ok(Self {
            schema_version: SCHEMA_V1,
            strategy: input.strategy,
            episode_id: input.episode_id,
            model_configuration: input.model_configuration,
            authorship_configuration: input.authorship_configuration,
            strategy_instruction: input.strategy_instruction,
            tool_catalog: oracle_debate_tool_catalog_id(input.strategy)?,
            tools,
            private_context: input.private_context,
            budget: input.budget,
        })
    }

    /// Returns the debate strategy assigned to this generic episode.
    #[must_use]
    pub const fn strategy(&self) -> OracleDebateStrategy {
        self.strategy
    }

    /// Returns the distinct durable episode identity.
    #[must_use]
    pub const fn episode_id(&self) -> EpisodeId {
        self.episode_id
    }

    /// Returns the frozen resolved model configuration.
    #[must_use]
    pub const fn model_configuration(&self) -> ContentId<ResolvedRuntimeModelArtifact> {
        self.model_configuration
    }

    /// Returns the verification-domain model identity required by model authorship records.
    #[must_use]
    pub const fn authorship_configuration(&self) -> ContentId<ModelConfigurationArtifact> {
        self.authorship_configuration
    }

    /// Returns the strategy-specific instruction block appended after common instructions.
    #[must_use]
    pub const fn strategy_instruction(&self) -> ContentId<InstructionBlock> {
        self.strategy_instruction
    }

    /// Returns the exact canonical tool catalog identity.
    #[must_use]
    pub const fn tool_catalog(&self) -> ContentId<ToolCatalog> {
        self.tool_catalog
    }

    /// Returns the server-enforced product capabilities in canonical order.
    #[must_use]
    pub fn tools(&self) -> &[OracleDebateTool] {
        &self.tools
    }

    /// Returns strategy-private submitted context roots in append order.
    #[must_use]
    pub fn private_context(&self) -> &[ContentId<ContextBlock>] {
        &self.private_context
    }

    /// Returns the independently enforced strategy budget.
    #[must_use]
    pub const fn budget(&self) -> &EpisodeBudget {
        &self.budget
    }

    fn validate(&self) -> Result<(), OracleModelDebatePlanError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleModelDebatePlanError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.tools != self.strategy.required_tools()
            || self.tool_catalog != oracle_debate_tool_catalog_id(self.strategy)?
        {
            return Err(OracleModelDebatePlanError::StrategyCapabilityMismatch);
        }
        validate_unique(&self.private_context, "strategy private context")
    }
}

impl TryFrom<OracleDebateEpisodeWire> for OracleDebateEpisodeV1 {
    type Error = OracleModelDebatePlanError;

    fn try_from(wire: OracleDebateEpisodeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            strategy: wire.strategy,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            authorship_configuration: wire.authorship_configuration,
            strategy_instruction: wire.strategy_instruction,
            tool_catalog: wire.tool_catalog,
            tools: wire.tools,
            private_context: wire.private_context,
            budget: wire.budget,
        };
        value.validate()?;
        Ok(value)
    }
}

impl From<OracleDebateEpisodeV1> for OracleDebateEpisodeWire {
    fn from(value: OracleDebateEpisodeV1) -> Self {
        Self {
            schema_version: value.schema_version,
            strategy: value.strategy,
            episode_id: value.episode_id,
            model_configuration: value.model_configuration,
            authorship_configuration: value.authorship_configuration,
            strategy_instruction: value.strategy_instruction,
            tool_catalog: value.tool_catalog,
            tools: value.tools,
            private_context: value.private_context,
            budget: value.budget,
        }
    }
}

/// Creates a strategy binding whose tool identity is derived from trusted product definitions.
///
/// # Errors
///
/// Rejects duplicate strategy-private context or an unrepresentable tool-catalog identity.
pub fn prepare_oracle_debate_episode(
    input: OracleDebateEpisodeInput,
) -> Result<OracleDebateEpisodeV1, OracleModelDebatePlanError> {
    OracleDebateEpisodeV1::new(input)
}

/// Constructor input for a complete two-episode `OracleModelDebate`.
pub struct OracleModelDebatePlanInput {
    /// Owning migration task.
    pub task_id: TaskId,
    /// Exact resolved task/source input artifact.
    pub task_inputs: ContentId<OracleTaskInputArtifact>,
    /// Original minimum structured caller declaration.
    pub declared_domain: ContentId<DeclaredDomainArtifact>,
    /// Trusted immutable admission policy selected for this search.
    pub admission_policy: ContentId<AdmissionPolicyArtifact>,
    /// Stable common instructions in deterministic order.
    pub common_instructions: Vec<ContentId<InstructionBlock>>,
    /// Stable caller/source/policy context in deterministic order.
    pub shared_context: Vec<ContentId<ContextBlock>>,
    /// Synthesis episode binding.
    pub synthesis: OracleDebateEpisodeV1,
    /// Adversarial episode binding.
    pub adversarial: OracleDebateEpisodeV1,
}

/// Immutable plan binding one task to isolated synthesis and adversarial Oracle Agent episodes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "OracleModelDebatePlanWire",
    into = "OracleModelDebatePlanWire"
)]
pub struct OracleModelDebatePlanV1 {
    schema_version: u16,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    admission_policy: ContentId<AdmissionPolicyArtifact>,
    common_instructions: Vec<ContentId<InstructionBlock>>,
    shared_context: Vec<ContentId<ContextBlock>>,
    synthesis: OracleDebateEpisodeV1,
    adversarial: OracleDebateEpisodeV1,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleModelDebatePlanWire {
    schema_version: u16,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    admission_policy: ContentId<AdmissionPolicyArtifact>,
    common_instructions: Vec<ContentId<InstructionBlock>>,
    shared_context: Vec<ContentId<ContextBlock>>,
    synthesis: OracleDebateEpisodeV1,
    adversarial: OracleDebateEpisodeV1,
}

impl OracleModelDebatePlanV1 {
    /// Creates one immutable two-strategy `OracleModelDebate` plan.
    ///
    /// # Errors
    ///
    /// Rejects empty/duplicated stable-prefix material, swapped strategies, reused episode identities,
    /// strategy-private context leaked into the shared prefix, or inconsistent strategy capabilities.
    pub fn new(input: OracleModelDebatePlanInput) -> Result<Self, OracleModelDebatePlanError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            task_id: input.task_id,
            task_inputs: input.task_inputs,
            declared_domain: input.declared_domain,
            admission_policy: input.admission_policy,
            common_instructions: input.common_instructions,
            shared_context: input.shared_context,
            synthesis: input.synthesis,
            adversarial: input.adversarial,
        };
        value.validate()?;
        Ok(value)
    }

    /// Returns the owning task.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the exact task/source inputs shared by both strategies.
    #[must_use]
    pub const fn task_inputs(&self) -> ContentId<OracleTaskInputArtifact> {
        self.task_inputs
    }

    /// Returns the original minimum structured caller declaration.
    #[must_use]
    pub const fn declared_domain(&self) -> ContentId<DeclaredDomainArtifact> {
        self.declared_domain
    }

    /// Returns the trusted admission policy selected before model work.
    #[must_use]
    pub const fn admission_policy(&self) -> ContentId<AdmissionPolicyArtifact> {
        self.admission_policy
    }

    /// Returns common instructions in their stable prefix order.
    #[must_use]
    pub fn common_instructions(&self) -> &[ContentId<InstructionBlock>] {
        &self.common_instructions
    }

    /// Returns immutable shared caller/source/policy context.
    #[must_use]
    pub fn shared_context(&self) -> &[ContentId<ContextBlock>] {
        &self.shared_context
    }

    /// Returns the isolated synthesis episode binding.
    #[must_use]
    pub const fn synthesis(&self) -> &OracleDebateEpisodeV1 {
        &self.synthesis
    }

    /// Returns the isolated adversarial episode binding.
    #[must_use]
    pub const fn adversarial(&self) -> &OracleDebateEpisodeV1 {
        &self.adversarial
    }

    fn validate(&self) -> Result<(), OracleModelDebatePlanError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleModelDebatePlanError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.common_instructions.is_empty() || self.shared_context.is_empty() {
            return Err(OracleModelDebatePlanError::EmptyStablePrefix);
        }
        validate_unique(&self.common_instructions, "common instructions")?;
        validate_unique(&self.shared_context, "shared context")?;
        self.synthesis.validate()?;
        self.adversarial.validate()?;
        if self.synthesis.strategy != OracleDebateStrategy::Synthesis
            || self.adversarial.strategy != OracleDebateStrategy::Adversarial
        {
            return Err(OracleModelDebatePlanError::StrategyBindingMismatch);
        }
        if self.synthesis.episode_id == self.adversarial.episode_id {
            return Err(OracleModelDebatePlanError::SharedEpisode);
        }
        let shared = self
            .shared_context
            .iter()
            .map(ContentId::to_wire)
            .collect::<HashSet<_>>();
        if self
            .synthesis
            .private_context
            .iter()
            .chain(&self.adversarial.private_context)
            .any(|context| shared.contains(&context.to_wire()))
        {
            return Err(OracleModelDebatePlanError::PrivateContextLeak);
        }
        Ok(())
    }
}

impl TryFrom<OracleModelDebatePlanWire> for OracleModelDebatePlanV1 {
    type Error = OracleModelDebatePlanError;

    fn try_from(wire: OracleModelDebatePlanWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            task_inputs: wire.task_inputs,
            declared_domain: wire.declared_domain,
            admission_policy: wire.admission_policy,
            common_instructions: wire.common_instructions,
            shared_context: wire.shared_context,
            synthesis: wire.synthesis,
            adversarial: wire.adversarial,
        };
        value.validate()?;
        Ok(value)
    }
}

impl From<OracleModelDebatePlanV1> for OracleModelDebatePlanWire {
    fn from(value: OracleModelDebatePlanV1) -> Self {
        Self {
            schema_version: value.schema_version,
            task_id: value.task_id,
            task_inputs: value.task_inputs,
            declared_domain: value.declared_domain,
            admission_policy: value.admission_policy,
            common_instructions: value.common_instructions,
            shared_context: value.shared_context,
            synthesis: value.synthesis,
            adversarial: value.adversarial,
        }
    }
}

/// Canonical strict tool-catalog bytes for one `OracleModelDebate` strategy.
///
/// # Errors
///
/// Returns an error only if canonical encoding fails.
pub fn oracle_debate_tool_catalog_bytes(
    strategy: OracleDebateStrategy,
) -> Result<Vec<u8>, OracleModelDebatePlanError> {
    let tools = crate::oracle_debate_tools::oracle_debate_tool_contracts(strategy)?;
    cairn_codec::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_V1,
        "strategy": strategy,
        "tools": tools,
    }))
    .map_err(|error| OracleModelDebatePlanError::Encoding(error.to_string()))
}

/// Derives the exact strategy tool-catalog identity from trusted product definitions.
///
/// # Errors
///
/// Returns an error when canonical encoding or identity derivation fails.
pub fn oracle_debate_tool_catalog_id(
    strategy: OracleDebateStrategy,
) -> Result<ContentId<ToolCatalog>, OracleModelDebatePlanError> {
    let bytes = oracle_debate_tool_catalog_bytes(strategy)?;
    ContentId::<ToolCatalog>::derive(&bytes)
        .map_err(|error| OracleModelDebatePlanError::Encoding(error.to_string()))
}

/// Archives the trusted strategy tool catalog and verifies that storage preserved its frozen identity.
///
/// # Errors
///
/// Returns an error when canonical encoding or content storage fails.
pub fn archive_oracle_debate_tool_catalog<S: ContentStore>(
    store: &mut S,
    strategy: OracleDebateStrategy,
) -> Result<ContentId<ToolCatalog>, OracleModelDebatePlanError> {
    let bytes = oracle_debate_tool_catalog_bytes(strategy)?;
    let expected = oracle_debate_tool_catalog_id(strategy)?;
    let descriptor = store.put::<ToolCatalog>(&mut Cursor::new(bytes))?;
    if descriptor.content_id != expected {
        return Err(OracleModelDebatePlanError::StrategyCapabilityMismatch);
    }
    Ok(descriptor.content_id)
}

fn validate_unique<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleModelDebatePlanError> {
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value.to_wire())) {
        return Err(OracleModelDebatePlanError::DuplicatePrefixEntry { field });
    }
    Ok(())
}

/// Invalid `OracleModelDebate` composition or strict V1 input.
#[derive(Debug, Error)]
pub enum OracleModelDebatePlanError {
    /// Trusted strategy catalog archival failed.
    #[error(transparent)]
    Storage(#[from] ContentStoreError),
    /// A schema other than the single current V1 was supplied.
    #[error("unsupported OracleModelDebate schema version {0}")]
    UnsupportedSchema(u16),
    /// Common strategy input must have both instruction and context material.
    #[error("OracleModelDebate stable prefix cannot be empty")]
    EmptyStablePrefix,
    /// One stable-prefix identity appeared twice.
    #[error("OracleModelDebate {field} contains a duplicate identity")]
    DuplicatePrefixEntry { field: &'static str },
    /// Persisted strategy capabilities or their catalog identity differ from trusted definitions.
    #[error("OracleModelDebate strategy capabilities do not match trusted product policy")]
    StrategyCapabilityMismatch,
    /// Synthesis and adversarial were placed in the wrong slots.
    #[error("OracleModelDebate synthesis/adversarial strategy binding is inconsistent")]
    StrategyBindingMismatch,
    /// Synthesis and adversarial reused one logical episode.
    #[error("OracleModelDebate synthesis and adversarial must use distinct episodes")]
    SharedEpisode,
    /// A strategy-private context identity was also placed in the shared prefix.
    #[error("OracleModelDebate strategy-private context leaked into shared context")]
    PrivateContextLeak,
    /// Canonical encoding or semantic identity derivation failed.
    #[error("OracleModelDebate encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use super::{
        OracleDebateEpisodeInput, OracleDebateStrategy, OracleDebateTool,
        OracleModelDebatePlanArtifact, OracleModelDebatePlanInput, OracleModelDebatePlanV1,
        prepare_oracle_debate_episode,
    };
    use cairn_agent::{
        ContextBlock, EpisodeBudget, InstructionBlock, ResolvedRuntimeModelArtifact,
    };
    use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
    use cairn_verification::{
        AdmissionPolicyArtifact, DeclaredDomainArtifact, ModelConfigurationArtifact,
        OracleTaskInputArtifact,
    };

    fn id<T: ContentType>(label: &str) -> ContentId<T> {
        ContentId::<T>::derive(label.as_bytes()).expect("identity")
    }

    fn strategy(strategy: OracleDebateStrategy, private: &[&str]) -> super::OracleDebateEpisodeV1 {
        prepare_oracle_debate_episode(OracleDebateEpisodeInput {
            strategy,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>(match strategy {
                OracleDebateStrategy::Synthesis => "synthesis-model",
                OracleDebateStrategy::Adversarial => "adversarial-model",
            }),
            authorship_configuration: id::<ModelConfigurationArtifact>(match strategy {
                OracleDebateStrategy::Synthesis => "synthesis-model",
                OracleDebateStrategy::Adversarial => "adversarial-model",
            }),
            strategy_instruction: id::<InstructionBlock>(match strategy {
                OracleDebateStrategy::Synthesis => "synthesis-instruction",
                OracleDebateStrategy::Adversarial => "adversarial-instruction",
            }),
            private_context: private
                .iter()
                .map(|value| id::<ContextBlock>(value))
                .collect(),
            budget: EpisodeBudget::default(),
        })
        .expect("strategy")
    }

    fn plan() -> OracleModelDebatePlanV1 {
        OracleModelDebatePlanV1::new(OracleModelDebatePlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared-domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller"), id::<ContextBlock>("source")],
            synthesis: strategy(OracleDebateStrategy::Synthesis, &["synthesis-private"]),
            adversarial: strategy(OracleDebateStrategy::Adversarial, &["adversarial-private"]),
        })
        .expect("plan")
    }

    #[test]
    fn plan_keeps_strategies_sessions_and_tools_isolated() {
        let plan = plan();
        assert_ne!(
            plan.synthesis().episode_id(),
            plan.adversarial().episode_id()
        );
        assert!(
            plan.synthesis()
                .tools()
                .contains(&OracleDebateTool::SearchExternalTests)
        );
        assert!(
            !plan
                .adversarial()
                .tools()
                .contains(&OracleDebateTool::SearchExternalTests)
        );
        assert_ne!(
            plan.synthesis().tool_catalog(),
            plan.adversarial().tool_catalog()
        );
    }

    #[test]
    fn strict_plan_round_trip_and_identity_bind_every_strategy_edge() {
        let plan = plan();
        let bytes = cairn_codec::to_vec(&plan).expect("encode");
        let decoded: OracleModelDebatePlanV1 = cairn_codec::from_slice(&bytes).expect("decode");
        assert_eq!(decoded, plan);
        let original = ContentId::<OracleModelDebatePlanArtifact>::derive(&bytes).expect("plan id");

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["synthesis"]["episode_id"] = value["adversarial"]["episode_id"].clone();
        assert!(serde_json::from_value::<OracleModelDebatePlanV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["synthesis"]["model_configuration"] =
            serde_json::to_value(id::<ResolvedRuntimeModelArtifact>("changed-model"))
                .expect("model id");
        let changed: OracleModelDebatePlanV1 = serde_json::from_value(value).expect("changed plan");
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed bytes");
        let changed_id =
            ContentId::<OracleModelDebatePlanArtifact>::derive(&changed_bytes).expect("changed id");
        assert_ne!(changed_id, original);
    }

    #[test]
    fn private_context_cannot_be_reclassified_as_shared() {
        let leaked = id::<ContextBlock>("leaked");
        let error = OracleModelDebatePlanV1::new(OracleModelDebatePlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared-domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![leaked],
            synthesis: strategy(OracleDebateStrategy::Synthesis, &["leaked"]),
            adversarial: strategy(OracleDebateStrategy::Adversarial, &["adversarial-private"]),
        })
        .expect_err("private context leak");
        assert!(error.to_string().contains("leaked"));
    }

    #[test]
    fn non_v1_and_unknown_fields_fail_closed() {
        let bytes = cairn_codec::to_vec(&plan()).expect("encode");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<OracleModelDebatePlanV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["legacy_session"] = serde_json::json!("forbidden");
        assert!(serde_json::from_value::<OracleModelDebatePlanV1>(value).is_err());
    }
}
