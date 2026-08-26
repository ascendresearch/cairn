//! Product-owned blue/red `OracleSearch` plan and cache-stable role projections.

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

/// Durable semantic domain for one complete `OracleSearch` plan.
pub enum OracleSearchPlanArtifact {}

impl ContentType for OracleSearchPlanArtifact {
    const DOMAIN: &'static str = "migration.oracle-search-plan.v1";
}

/// Closed product role used to bind generic agent episodes to `OracleSearch` authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleAgentRole {
    /// Domain/reference/property/corpus author.
    Blue,
    /// False-accept and false-reject breaker.
    Red,
}

/// Product tool capability offered to one `OracleSearch` role.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OracleRoleTool {
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

impl OracleAgentRole {
    const fn required_tools(self) -> &'static [OracleRoleTool] {
        match self {
            Self::Blue => &[
                OracleRoleTool::SearchExternalTests,
                OracleRoleTool::SubmitDomainRefinement,
                OracleRoleTool::SubmitOracleProposal,
            ],
            Self::Red => &[
                OracleRoleTool::SubmitCorrectVariant,
                OracleRoleTool::SubmitWrongVariant,
                OracleRoleTool::SubmitAdversarialCase,
            ],
        }
    }
}

/// Constructor input for one role-bound generic agent episode.
pub struct OracleRoleEpisodeInput {
    /// Closed `OracleSearch` role.
    pub role: OracleAgentRole,
    /// Distinct durable generic agent episode.
    pub episode_id: EpisodeId,
    /// Frozen resolved model configuration used by the episode.
    pub model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    /// Verification-domain identity of the same frozen model configuration for authorship edges.
    pub authorship_configuration: ContentId<ModelConfigurationArtifact>,
    /// One stable role-specific instruction block after common instructions.
    pub role_instruction: ContentId<InstructionBlock>,
    /// Optional role-private submitted context roots in append order.
    pub private_context: Vec<ContentId<ContextBlock>>,
    /// Independently enforced durable budget for this role episode.
    pub budget: EpisodeBudget,
}

/// Exact role, model, prompt, tool, and private-context binding for one episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleRoleEpisodeWire", into = "OracleRoleEpisodeWire")]
pub struct OracleRoleEpisodeV1 {
    schema_version: u16,
    role: OracleAgentRole,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    authorship_configuration: ContentId<ModelConfigurationArtifact>,
    role_instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    tools: Vec<OracleRoleTool>,
    private_context: Vec<ContentId<ContextBlock>>,
    budget: EpisodeBudget,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleRoleEpisodeWire {
    schema_version: u16,
    role: OracleAgentRole,
    episode_id: EpisodeId,
    model_configuration: ContentId<ResolvedRuntimeModelArtifact>,
    authorship_configuration: ContentId<ModelConfigurationArtifact>,
    role_instruction: ContentId<InstructionBlock>,
    tool_catalog: ContentId<ToolCatalog>,
    tools: Vec<OracleRoleTool>,
    private_context: Vec<ContentId<ContextBlock>>,
    budget: EpisodeBudget,
}

impl OracleRoleEpisodeV1 {
    fn new(input: OracleRoleEpisodeInput) -> Result<Self, OracleSearchPlanError> {
        validate_unique(&input.private_context, "role private context")?;
        let tools = input.role.required_tools().to_vec();
        Ok(Self {
            schema_version: SCHEMA_V1,
            role: input.role,
            episode_id: input.episode_id,
            model_configuration: input.model_configuration,
            authorship_configuration: input.authorship_configuration,
            role_instruction: input.role_instruction,
            tool_catalog: oracle_role_tool_catalog_id(input.role)?,
            tools,
            private_context: input.private_context,
            budget: input.budget,
        })
    }

    /// Returns the product role assigned to this generic episode.
    #[must_use]
    pub const fn role(&self) -> OracleAgentRole {
        self.role
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

    /// Returns the role-specific instruction block appended after common instructions.
    #[must_use]
    pub const fn role_instruction(&self) -> ContentId<InstructionBlock> {
        self.role_instruction
    }

    /// Returns the exact canonical tool catalog identity.
    #[must_use]
    pub const fn tool_catalog(&self) -> ContentId<ToolCatalog> {
        self.tool_catalog
    }

    /// Returns the server-enforced product capabilities in canonical order.
    #[must_use]
    pub fn tools(&self) -> &[OracleRoleTool] {
        &self.tools
    }

    /// Returns role-private submitted context roots in append order.
    #[must_use]
    pub fn private_context(&self) -> &[ContentId<ContextBlock>] {
        &self.private_context
    }

    /// Returns the independently enforced role budget.
    #[must_use]
    pub const fn budget(&self) -> &EpisodeBudget {
        &self.budget
    }

    fn validate(&self) -> Result<(), OracleSearchPlanError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleSearchPlanError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.tools != self.role.required_tools()
            || self.tool_catalog != oracle_role_tool_catalog_id(self.role)?
        {
            return Err(OracleSearchPlanError::RoleCapabilityMismatch);
        }
        validate_unique(&self.private_context, "role private context")
    }
}

impl TryFrom<OracleRoleEpisodeWire> for OracleRoleEpisodeV1 {
    type Error = OracleSearchPlanError;

    fn try_from(wire: OracleRoleEpisodeWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            role: wire.role,
            episode_id: wire.episode_id,
            model_configuration: wire.model_configuration,
            authorship_configuration: wire.authorship_configuration,
            role_instruction: wire.role_instruction,
            tool_catalog: wire.tool_catalog,
            tools: wire.tools,
            private_context: wire.private_context,
            budget: wire.budget,
        };
        value.validate()?;
        Ok(value)
    }
}

impl From<OracleRoleEpisodeV1> for OracleRoleEpisodeWire {
    fn from(value: OracleRoleEpisodeV1) -> Self {
        Self {
            schema_version: value.schema_version,
            role: value.role,
            episode_id: value.episode_id,
            model_configuration: value.model_configuration,
            authorship_configuration: value.authorship_configuration,
            role_instruction: value.role_instruction,
            tool_catalog: value.tool_catalog,
            tools: value.tools,
            private_context: value.private_context,
            budget: value.budget,
        }
    }
}

/// Creates a role binding whose tool identity is derived from trusted product definitions.
///
/// # Errors
///
/// Rejects duplicate role-private context or an unrepresentable tool-catalog identity.
pub fn prepare_oracle_role_episode(
    input: OracleRoleEpisodeInput,
) -> Result<OracleRoleEpisodeV1, OracleSearchPlanError> {
    OracleRoleEpisodeV1::new(input)
}

/// Constructor input for a complete two-episode `OracleSearch`.
pub struct OracleSearchPlanInput {
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
    /// Blue episode binding.
    pub blue: OracleRoleEpisodeV1,
    /// Red episode binding.
    pub red: OracleRoleEpisodeV1,
}

/// Immutable plan binding one task to isolated blue and red Oracle Agent episodes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleSearchPlanWire", into = "OracleSearchPlanWire")]
pub struct OracleSearchPlanV1 {
    schema_version: u16,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    admission_policy: ContentId<AdmissionPolicyArtifact>,
    common_instructions: Vec<ContentId<InstructionBlock>>,
    shared_context: Vec<ContentId<ContextBlock>>,
    blue: OracleRoleEpisodeV1,
    red: OracleRoleEpisodeV1,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleSearchPlanWire {
    schema_version: u16,
    task_id: TaskId,
    task_inputs: ContentId<OracleTaskInputArtifact>,
    declared_domain: ContentId<DeclaredDomainArtifact>,
    admission_policy: ContentId<AdmissionPolicyArtifact>,
    common_instructions: Vec<ContentId<InstructionBlock>>,
    shared_context: Vec<ContentId<ContextBlock>>,
    blue: OracleRoleEpisodeV1,
    red: OracleRoleEpisodeV1,
}

impl OracleSearchPlanV1 {
    /// Creates one immutable two-role `OracleSearch` plan.
    ///
    /// # Errors
    ///
    /// Rejects empty/duplicated stable-prefix material, swapped roles, reused episode identities,
    /// role-private context leaked into the shared prefix, or inconsistent role capabilities.
    pub fn new(input: OracleSearchPlanInput) -> Result<Self, OracleSearchPlanError> {
        let value = Self {
            schema_version: SCHEMA_V1,
            task_id: input.task_id,
            task_inputs: input.task_inputs,
            declared_domain: input.declared_domain,
            admission_policy: input.admission_policy,
            common_instructions: input.common_instructions,
            shared_context: input.shared_context,
            blue: input.blue,
            red: input.red,
        };
        value.validate()?;
        Ok(value)
    }

    /// Returns the owning task.
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    /// Returns the exact task/source inputs shared by both roles.
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

    /// Returns the isolated blue episode binding.
    #[must_use]
    pub const fn blue(&self) -> &OracleRoleEpisodeV1 {
        &self.blue
    }

    /// Returns the isolated red episode binding.
    #[must_use]
    pub const fn red(&self) -> &OracleRoleEpisodeV1 {
        &self.red
    }

    fn validate(&self) -> Result<(), OracleSearchPlanError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OracleSearchPlanError::UnsupportedSchema(
                self.schema_version,
            ));
        }
        if self.common_instructions.is_empty() || self.shared_context.is_empty() {
            return Err(OracleSearchPlanError::EmptyStablePrefix);
        }
        validate_unique(&self.common_instructions, "common instructions")?;
        validate_unique(&self.shared_context, "shared context")?;
        self.blue.validate()?;
        self.red.validate()?;
        if self.blue.role != OracleAgentRole::Blue || self.red.role != OracleAgentRole::Red {
            return Err(OracleSearchPlanError::RoleBindingMismatch);
        }
        if self.blue.episode_id == self.red.episode_id {
            return Err(OracleSearchPlanError::SharedEpisode);
        }
        let shared = self
            .shared_context
            .iter()
            .map(ContentId::to_wire)
            .collect::<HashSet<_>>();
        if self
            .blue
            .private_context
            .iter()
            .chain(&self.red.private_context)
            .any(|context| shared.contains(&context.to_wire()))
        {
            return Err(OracleSearchPlanError::PrivateContextLeak);
        }
        Ok(())
    }
}

impl TryFrom<OracleSearchPlanWire> for OracleSearchPlanV1 {
    type Error = OracleSearchPlanError;

    fn try_from(wire: OracleSearchPlanWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            task_id: wire.task_id,
            task_inputs: wire.task_inputs,
            declared_domain: wire.declared_domain,
            admission_policy: wire.admission_policy,
            common_instructions: wire.common_instructions,
            shared_context: wire.shared_context,
            blue: wire.blue,
            red: wire.red,
        };
        value.validate()?;
        Ok(value)
    }
}

impl From<OracleSearchPlanV1> for OracleSearchPlanWire {
    fn from(value: OracleSearchPlanV1) -> Self {
        Self {
            schema_version: value.schema_version,
            task_id: value.task_id,
            task_inputs: value.task_inputs,
            declared_domain: value.declared_domain,
            admission_policy: value.admission_policy,
            common_instructions: value.common_instructions,
            shared_context: value.shared_context,
            blue: value.blue,
            red: value.red,
        }
    }
}

/// Canonical strict tool-catalog bytes for one `OracleSearch` role.
///
/// # Errors
///
/// Returns an error only if canonical encoding fails.
pub fn oracle_role_tool_catalog_bytes(
    role: OracleAgentRole,
) -> Result<Vec<u8>, OracleSearchPlanError> {
    let tools = crate::oracle_tools::oracle_role_tool_contracts(role)?;
    cairn_codec::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_V1,
        "role": role,
        "tools": tools,
    }))
    .map_err(|error| OracleSearchPlanError::Encoding(error.to_string()))
}

/// Derives the exact role tool-catalog identity from trusted product definitions.
///
/// # Errors
///
/// Returns an error when canonical encoding or identity derivation fails.
pub fn oracle_role_tool_catalog_id(
    role: OracleAgentRole,
) -> Result<ContentId<ToolCatalog>, OracleSearchPlanError> {
    let bytes = oracle_role_tool_catalog_bytes(role)?;
    ContentId::<ToolCatalog>::derive(&bytes)
        .map_err(|error| OracleSearchPlanError::Encoding(error.to_string()))
}

/// Archives the trusted role tool catalog and verifies that storage preserved its frozen identity.
///
/// # Errors
///
/// Returns an error when canonical encoding or content storage fails.
pub fn archive_oracle_role_tool_catalog<S: ContentStore>(
    store: &mut S,
    role: OracleAgentRole,
) -> Result<ContentId<ToolCatalog>, OracleSearchPlanError> {
    let bytes = oracle_role_tool_catalog_bytes(role)?;
    let expected = oracle_role_tool_catalog_id(role)?;
    let descriptor = store.put::<ToolCatalog>(&mut Cursor::new(bytes))?;
    if descriptor.content_id != expected {
        return Err(OracleSearchPlanError::RoleCapabilityMismatch);
    }
    Ok(descriptor.content_id)
}

fn validate_unique<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OracleSearchPlanError> {
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value.to_wire())) {
        return Err(OracleSearchPlanError::DuplicatePrefixEntry { field });
    }
    Ok(())
}

/// Invalid `OracleSearch` composition or strict V1 input.
#[derive(Debug, Error)]
pub enum OracleSearchPlanError {
    /// Trusted role catalog archival failed.
    #[error(transparent)]
    Storage(#[from] ContentStoreError),
    /// A schema other than the single current V1 was supplied.
    #[error("unsupported OracleSearch schema version {0}")]
    UnsupportedSchema(u16),
    /// Common role input must have both instruction and context material.
    #[error("OracleSearch stable prefix cannot be empty")]
    EmptyStablePrefix,
    /// One stable-prefix identity appeared twice.
    #[error("OracleSearch {field} contains a duplicate identity")]
    DuplicatePrefixEntry { field: &'static str },
    /// Persisted role capabilities or their catalog identity differ from trusted definitions.
    #[error("OracleSearch role capabilities do not match trusted product policy")]
    RoleCapabilityMismatch,
    /// Blue and red were placed in the wrong slots.
    #[error("OracleSearch blue/red role binding is inconsistent")]
    RoleBindingMismatch,
    /// Blue and red reused one logical episode.
    #[error("OracleSearch blue and red must use distinct episodes")]
    SharedEpisode,
    /// A role-private context identity was also placed in the shared prefix.
    #[error("OracleSearch role-private context leaked into shared context")]
    PrivateContextLeak,
    /// Canonical encoding or semantic identity derivation failed.
    #[error("OracleSearch encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use super::{
        OracleAgentRole, OracleRoleEpisodeInput, OracleRoleTool, OracleSearchPlanArtifact,
        OracleSearchPlanInput, OracleSearchPlanV1, prepare_oracle_role_episode,
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

    fn role(role: OracleAgentRole, private: &[&str]) -> super::OracleRoleEpisodeV1 {
        prepare_oracle_role_episode(OracleRoleEpisodeInput {
            role,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>(match role {
                OracleAgentRole::Blue => "blue-model",
                OracleAgentRole::Red => "red-model",
            }),
            authorship_configuration: id::<ModelConfigurationArtifact>(match role {
                OracleAgentRole::Blue => "blue-model",
                OracleAgentRole::Red => "red-model",
            }),
            role_instruction: id::<InstructionBlock>(match role {
                OracleAgentRole::Blue => "blue-instruction",
                OracleAgentRole::Red => "red-instruction",
            }),
            private_context: private
                .iter()
                .map(|value| id::<ContextBlock>(value))
                .collect(),
            budget: EpisodeBudget::default(),
        })
        .expect("role")
    }

    fn plan() -> OracleSearchPlanV1 {
        OracleSearchPlanV1::new(OracleSearchPlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared-domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller"), id::<ContextBlock>("source")],
            blue: role(OracleAgentRole::Blue, &["blue-private"]),
            red: role(OracleAgentRole::Red, &["red-private"]),
        })
        .expect("plan")
    }

    #[test]
    fn plan_keeps_roles_sessions_and_tools_isolated() {
        let plan = plan();
        assert_ne!(plan.blue().episode_id(), plan.red().episode_id());
        assert!(
            plan.blue()
                .tools()
                .contains(&OracleRoleTool::SearchExternalTests)
        );
        assert!(
            !plan
                .red()
                .tools()
                .contains(&OracleRoleTool::SearchExternalTests)
        );
        assert_ne!(plan.blue().tool_catalog(), plan.red().tool_catalog());
    }

    #[test]
    fn strict_plan_round_trip_and_identity_bind_every_role_edge() {
        let plan = plan();
        let bytes = cairn_codec::to_vec(&plan).expect("encode");
        let decoded: OracleSearchPlanV1 = cairn_codec::from_slice(&bytes).expect("decode");
        assert_eq!(decoded, plan);
        let original = ContentId::<OracleSearchPlanArtifact>::derive(&bytes).expect("plan id");

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["blue"]["episode_id"] = value["red"]["episode_id"].clone();
        assert!(serde_json::from_value::<OracleSearchPlanV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["blue"]["model_configuration"] =
            serde_json::to_value(id::<ResolvedRuntimeModelArtifact>("changed-model"))
                .expect("model id");
        let changed: OracleSearchPlanV1 = serde_json::from_value(value).expect("changed plan");
        let changed_bytes = cairn_codec::to_vec(&changed).expect("changed bytes");
        let changed_id =
            ContentId::<OracleSearchPlanArtifact>::derive(&changed_bytes).expect("changed id");
        assert_ne!(changed_id, original);
    }

    #[test]
    fn private_context_cannot_be_reclassified_as_shared() {
        let leaked = id::<ContextBlock>("leaked");
        let error = OracleSearchPlanV1::new(OracleSearchPlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task-inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared-domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![leaked],
            blue: role(OracleAgentRole::Blue, &["leaked"]),
            red: role(OracleAgentRole::Red, &["red-private"]),
        })
        .expect_err("private context leak");
        assert!(error.to_string().contains("leaked"));
    }

    #[test]
    fn non_v1_and_unknown_fields_fail_closed() {
        let bytes = cairn_codec::to_vec(&plan()).expect("encode");
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["schema_version"] = serde_json::json!(2);
        assert!(serde_json::from_value::<OracleSearchPlanV1>(value).is_err());

        let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
        value["legacy_session"] = serde_json::json!("forbidden");
        assert!(serde_json::from_value::<OracleSearchPlanV1>(value).is_err());
    }
}
