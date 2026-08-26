//! Cache-stable, content-audited model input projection for Oracle Agent roles.

use std::{collections::HashSet, io::Cursor};

use cairn_agent::{
    ContextBlock, HistoryItem, InstructionBlock, ModelName, ModelOutputTokenLimit, ModelSelection,
    NativeRequestSpec, PolicyDocument, ToolCatalog, TurnInputDecision,
};
use cairn_protocol::{ContentId, ContentType};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    OracleAgentRole, OracleSearchPlanArtifact, OracleSearchPlanError, OracleSearchPlanV1,
    oracle_role_native_tools, oracle_role_tool_catalog_id,
};

const SCHEMA_V1: u16 = 1;

/// Semantic domain for one exact role-visible prompt projection.
pub enum OracleRolePromptArtifact {}

impl ContentType for OracleRolePromptArtifact {
    const DOMAIN: &'static str = "migration.oracle-role-prompt.v1";
}

/// Constructor input for one role turn. Existing evidence is append-only; diagnostics and the
/// current request form the mutable suffix.
pub struct OracleRolePromptInput {
    /// Frozen role selected from the plan.
    pub role: OracleAgentRole,
    /// Submitted evidence visible to this role in append order.
    pub append_only_context: Vec<ContentId<ContextBlock>>,
    /// Current trusted diagnostic artifacts in deterministic order.
    pub diagnostic_context: Vec<ContentId<ContextBlock>>,
    /// Current role request; prior native history remains in the durable continuation.
    pub current_request: ContentId<HistoryItem>,
    /// Generic agent/network/data policy visible to input audit.
    pub policy: ContentId<PolicyDocument>,
}

/// Exact ordered prompt projection. Stable fields precede append-only evidence and the current
/// diagnostic/request suffix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "OracleRolePromptWire", into = "OracleRolePromptWire")]
pub struct OracleRolePromptV1 {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    role: OracleAgentRole,
    instructions: Vec<ContentId<InstructionBlock>>,
    stable_context: Vec<ContentId<ContextBlock>>,
    append_only_context: Vec<ContentId<ContextBlock>>,
    diagnostic_context: Vec<ContentId<ContextBlock>>,
    current_request: ContentId<HistoryItem>,
    policy: ContentId<PolicyDocument>,
    tool_catalog: ContentId<ToolCatalog>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleRolePromptWire {
    schema_version: u16,
    search_plan: ContentId<OracleSearchPlanArtifact>,
    role: OracleAgentRole,
    instructions: Vec<ContentId<InstructionBlock>>,
    stable_context: Vec<ContentId<ContextBlock>>,
    append_only_context: Vec<ContentId<ContextBlock>>,
    diagnostic_context: Vec<ContentId<ContextBlock>>,
    current_request: ContentId<HistoryItem>,
    policy: ContentId<PolicyDocument>,
    tool_catalog: ContentId<ToolCatalog>,
}

impl OracleRolePromptV1 {
    /// Returns the frozen role.
    #[must_use]
    pub const fn role(&self) -> OracleAgentRole {
        self.role
    }

    /// Returns the stable instruction prefix in exact order.
    #[must_use]
    pub fn instructions(&self) -> &[ContentId<InstructionBlock>] {
        &self.instructions
    }

    /// Returns caller/source/policy and role-private stable context in exact order.
    #[must_use]
    pub fn stable_context(&self) -> &[ContentId<ContextBlock>] {
        &self.stable_context
    }

    /// Returns the append-only evidence suffix.
    #[must_use]
    pub fn append_only_context(&self) -> &[ContentId<ContextBlock>] {
        &self.append_only_context
    }

    /// Builds the generic audited decision without changing semantic order.
    #[must_use]
    pub fn turn_input_decision(&self, selection: ModelSelection) -> TurnInputDecision {
        let mut context = self.stable_context.clone();
        context.extend_from_slice(&self.append_only_context);
        context.extend_from_slice(&self.diagnostic_context);
        TurnInputDecision {
            selection,
            instructions: self.instructions.clone(),
            tool_catalog: self.tool_catalog,
            history: vec![self.current_request],
            context,
            pending_results: Vec::new(),
            policy: self.policy,
        }
    }

    fn validate_shape(&self) -> Result<(), OraclePromptError> {
        if self.schema_version != SCHEMA_V1 {
            return Err(OraclePromptError::UnsupportedSchema(self.schema_version));
        }
        if self.instructions.is_empty() || self.stable_context.is_empty() {
            return Err(OraclePromptError::EmptyStablePrefix);
        }
        unique(&self.instructions, "instructions")?;
        let contexts = self
            .stable_context
            .iter()
            .chain(&self.append_only_context)
            .chain(&self.diagnostic_context)
            .map(ContentId::to_wire)
            .collect::<Vec<_>>();
        if contexts.iter().collect::<HashSet<_>>().len() != contexts.len() {
            return Err(OraclePromptError::DuplicateEntry("context"));
        }
        if self.tool_catalog != oracle_role_tool_catalog_id(self.role)? {
            return Err(OraclePromptError::ToolCatalogMismatch);
        }
        Ok(())
    }
}

impl TryFrom<OracleRolePromptWire> for OracleRolePromptV1 {
    type Error = OraclePromptError;

    fn try_from(wire: OracleRolePromptWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            search_plan: wire.search_plan,
            role: wire.role,
            instructions: wire.instructions,
            stable_context: wire.stable_context,
            append_only_context: wire.append_only_context,
            diagnostic_context: wire.diagnostic_context,
            current_request: wire.current_request,
            policy: wire.policy,
            tool_catalog: wire.tool_catalog,
        };
        value.validate_shape()?;
        Ok(value)
    }
}

impl From<OracleRolePromptV1> for OracleRolePromptWire {
    fn from(value: OracleRolePromptV1) -> Self {
        Self {
            schema_version: value.schema_version,
            search_plan: value.search_plan,
            role: value.role,
            instructions: value.instructions,
            stable_context: value.stable_context,
            append_only_context: value.append_only_context,
            diagnostic_context: value.diagnostic_context,
            current_request: value.current_request,
            policy: value.policy,
            tool_catalog: value.tool_catalog,
        }
    }
}

/// Builds the exact role projection from a frozen search plan.
///
/// # Errors
///
/// Rejects duplicates, role mismatch, invalid plan identity, or a changed trusted catalog.
pub fn prepare_oracle_role_prompt(
    plan: &OracleSearchPlanV1,
    input: OracleRolePromptInput,
) -> Result<OracleRolePromptV1, OraclePromptError> {
    let role = match input.role {
        OracleAgentRole::Blue => plan.blue(),
        OracleAgentRole::Red => plan.red(),
    };
    if role.role() != input.role {
        return Err(OraclePromptError::RoleMismatch);
    }
    let mut instructions = plan.common_instructions().to_vec();
    instructions.push(role.role_instruction());
    let mut stable_context = plan.shared_context().to_vec();
    stable_context.extend_from_slice(role.private_context());
    let plan_bytes = cairn_codec::to_vec(plan)
        .map_err(|error| OraclePromptError::Encoding(error.to_string()))?;
    let value = OracleRolePromptV1 {
        schema_version: SCHEMA_V1,
        search_plan: ContentId::derive(&plan_bytes)
            .map_err(|error| OraclePromptError::Encoding(error.to_string()))?,
        role: input.role,
        instructions,
        stable_context,
        append_only_context: input.append_only_context,
        diagnostic_context: input.diagnostic_context,
        current_request: input.current_request,
        policy: input.policy,
        tool_catalog: role.tool_catalog(),
    };
    value.validate_shape()?;
    Ok(value)
}

/// Fully materialized, secret-free native prompt segments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializedOraclePrompt {
    instructions: String,
    user_text: String,
}

impl MaterializedOraclePrompt {
    /// Returns the stable system/developer text.
    #[must_use]
    pub fn instructions(&self) -> &str {
        &self.instructions
    }

    /// Returns stable context followed by append-only evidence and the current suffix.
    #[must_use]
    pub fn user_text(&self) -> &str {
        &self.user_text
    }

    /// Builds the native request spec with the exact role catalog.
    ///
    /// # Errors
    ///
    /// Returns an error only if trusted role tool definitions cannot be represented.
    pub fn native_spec(
        &self,
        role: OracleAgentRole,
        wire_model: ModelName,
        max_output_tokens: ModelOutputTokenLimit,
    ) -> Result<NativeRequestSpec, OraclePromptError> {
        Ok(NativeRequestSpec {
            wire_model,
            instructions: self.instructions.clone(),
            tools: oracle_role_native_tools(role)?,
            max_output_tokens,
        })
    }
}

/// Resolves every prompt identity from CAS and emits canonical native text segments.
///
/// # Errors
///
/// Fails closed on absent, corrupt, or non-JSON content.
pub fn materialize_oracle_prompt<S: ContentStore>(
    store: &S,
    prompt: &OracleRolePromptV1,
) -> Result<MaterializedOraclePrompt, OraclePromptError> {
    prompt.validate_shape()?;
    let instructions = resolve_many(store, &prompt.instructions)?;
    let stable_context = resolve_many(store, &prompt.stable_context)?;
    let append_only_context = resolve_many(store, &prompt.append_only_context)?;
    let diagnostics = resolve_many(store, &prompt.diagnostic_context)?;
    let request = resolve_one(store, &prompt.current_request)?;
    let instructions = cairn_codec::to_vec(&json!({
        "oracle_agent_instructions": instructions,
    }))
    .map_err(|error| OraclePromptError::Encoding(error.to_string()))?;
    let user_text = cairn_codec::to_vec(&json!({
        "stable_context": stable_context,
        "append_only_evidence": append_only_context,
        "diagnostics": diagnostics,
        "current_request": request,
    }))
    .map_err(|error| OraclePromptError::Encoding(error.to_string()))?;
    Ok(MaterializedOraclePrompt {
        instructions: String::from_utf8(instructions)
            .map_err(|error| OraclePromptError::Encoding(error.to_string()))?,
        user_text: String::from_utf8(user_text)
            .map_err(|error| OraclePromptError::Encoding(error.to_string()))?,
    })
}

fn resolve_many<T: ContentType, S: ContentStore>(
    store: &S,
    ids: &[ContentId<T>],
) -> Result<Vec<Value>, OraclePromptError> {
    ids.iter().map(|id| resolve_one(store, id)).collect()
}

fn resolve_one<T: ContentType, S: ContentStore>(
    store: &S,
    id: &ContentId<T>,
) -> Result<Value, OraclePromptError> {
    let mut bytes = Vec::new();
    store.write_to(id, &mut bytes)?;
    cairn_codec::from_slice(&bytes).map_err(|error| OraclePromptError::Encoding(error.to_string()))
}

fn unique<T: ContentType>(
    values: &[ContentId<T>],
    field: &'static str,
) -> Result<(), OraclePromptError> {
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value.to_wire())) {
        return Err(OraclePromptError::DuplicateEntry(field));
    }
    Ok(())
}

/// Archives the canonical prompt artifact and returns its typed identity.
///
/// # Errors
///
/// Returns storage or encoding errors without synthesizing replacement content.
pub fn archive_oracle_role_prompt<S: ContentStore>(
    store: &mut S,
    prompt: &OracleRolePromptV1,
) -> Result<ContentId<OracleRolePromptArtifact>, OraclePromptError> {
    let bytes = cairn_codec::to_vec(prompt)
        .map_err(|error| OraclePromptError::Encoding(error.to_string()))?;
    Ok(store
        .put::<OracleRolePromptArtifact>(&mut Cursor::new(bytes))?
        .content_id)
}

/// Invalid Oracle Agent prompt projection or materialization.
#[derive(Debug, Error)]
pub enum OraclePromptError {
    /// A schema other than the current V1 was supplied.
    #[error("unsupported Oracle Agent prompt schema version {0}")]
    UnsupportedSchema(u16),
    /// Stable instruction/context material is required.
    #[error("Oracle Agent stable prefix cannot be empty")]
    EmptyStablePrefix,
    /// An identity appeared twice across a semantic prompt section.
    #[error("Oracle Agent prompt contains duplicate {0}")]
    DuplicateEntry(&'static str),
    /// The requested role does not match its frozen plan slot.
    #[error("Oracle Agent prompt role does not match the search plan")]
    RoleMismatch,
    /// Tool catalog is not the exact trusted role catalog.
    #[error("Oracle Agent prompt tool catalog does not match role policy")]
    ToolCatalogMismatch,
    /// Search-plan contract failure.
    #[error(transparent)]
    SearchPlan(#[from] OracleSearchPlanError),
    /// Trusted tool definition failure.
    #[error(transparent)]
    Tool(#[from] crate::OracleToolError),
    /// Referenced content could not be loaded or verified.
    #[error(transparent)]
    Storage(#[from] ContentStoreError),
    /// Canonical encoding failed.
    #[error("Oracle Agent prompt encoding failed: {0}")]
    Encoding(String),
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use cairn_agent::{
        ContextBlock, EpisodeBudget, HistoryItem, InstructionBlock, ModelName,
        ModelOutputTokenLimit, ModelProtocolConfig, NativeProtocolCodec, NativeToolResult,
        PolicyDocument, ProviderToolCallId, ResolvedRuntimeModelArtifact, ResponsesReasoningReplay,
    };
    use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
    use cairn_record::ContentStore;
    use cairn_store_sqlite::SqliteContentStore;
    use cairn_verification::{
        AdmissionPolicyArtifact, DeclaredDomainArtifact, ModelConfigurationArtifact,
        OracleTaskInputArtifact,
    };

    use super::{OracleRolePromptInput, materialize_oracle_prompt, prepare_oracle_role_prompt};
    use crate::{
        OracleAgentRole, OracleRoleEpisodeInput, OracleSearchPlanInput, OracleSearchPlanV1,
        archive_oracle_role_tool_catalog, prepare_oracle_role_episode,
    };

    fn put<T: ContentType>(
        store: &mut SqliteContentStore,
        value: &serde_json::Value,
    ) -> ContentId<T> {
        let bytes = cairn_codec::to_vec(&value).expect("canonical JSON");
        store
            .put::<T>(&mut Cursor::new(bytes))
            .expect("put content")
            .content_id
    }

    fn role(store: &mut SqliteContentStore, role: OracleAgentRole) -> crate::OracleRoleEpisodeV1 {
        let role_instruction = put::<InstructionBlock>(
            store,
            &serde_json::json!({"text": match role {
                OracleAgentRole::Blue => "author proposals and cite evidence",
                OracleAgentRole::Red => "break the frozen proposal"
            }}),
        );
        let catalog_id = archive_oracle_role_tool_catalog(store, role).expect("catalog");
        let prepared = prepare_oracle_role_episode(OracleRoleEpisodeInput {
            role,
            episode_id: EpisodeId::new(),
            model_configuration: ContentId::<ResolvedRuntimeModelArtifact>::derive(
                format!("{role:?}-runtime").as_bytes(),
            )
            .expect("runtime model"),
            authorship_configuration: ContentId::<ModelConfigurationArtifact>::derive(
                format!("{role:?}-authorship").as_bytes(),
            )
            .expect("authorship model"),
            role_instruction,
            private_context: Vec::new(),
            budget: EpisodeBudget::default(),
        })
        .expect("role");
        assert_eq!(prepared.tool_catalog(), catalog_id);
        prepared
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the restart control keeps exact role materialization, native tool decoding, CAS recovery, and prefix comparison in one evidence path"
    )]
    fn blue_native_prefix_and_catalog_survive_second_turn_restart() {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("content.db");
        let cas = directory.path().join("cas");
        let mut store = SqliteContentStore::open(&database, &cas).expect("store");
        let common = put::<InstructionBlock>(
            &mut store,
            &serde_json::json!({"text":"preserve provenance and never self-admit"}),
        );
        let caller = put::<ContextBlock>(
            &mut store,
            &serde_json::json!({"kind":"caller-domain","unknowns":["target rounding"]}),
        );
        let source = put::<ContextBlock>(
            &mut store,
            &serde_json::json!({"kind":"source-snapshot","entry":"reduce"}),
        );
        let evidence = put::<ContextBlock>(
            &mut store,
            &serde_json::json!({"kind":"mandatory-cases","count":2}),
        );
        let request = put::<HistoryItem>(
            &mut store,
            &serde_json::json!({"role":"user","content":"prepare the first oracle proposal"}),
        );
        let policy = put::<PolicyDocument>(
            &mut store,
            &serde_json::json!({"network":"approved-repositories-only"}),
        );
        let blue = role(&mut store, OracleAgentRole::Blue);
        let red = role(&mut store, OracleAgentRole::Red);
        let plan = OracleSearchPlanV1::new(OracleSearchPlanInput {
            task_id: TaskId::new(),
            task_inputs: ContentId::<OracleTaskInputArtifact>::derive(b"task-inputs")
                .expect("task inputs"),
            declared_domain: ContentId::<DeclaredDomainArtifact>::derive(b"domain")
                .expect("domain"),
            admission_policy: ContentId::<AdmissionPolicyArtifact>::derive(b"admission-policy")
                .expect("admission policy"),
            common_instructions: vec![common],
            shared_context: vec![caller, source],
            blue,
            red,
        })
        .expect("plan");
        let projection = prepare_oracle_role_prompt(
            &plan,
            OracleRolePromptInput {
                role: OracleAgentRole::Blue,
                append_only_context: vec![evidence],
                diagnostic_context: Vec::new(),
                current_request: request,
                policy,
            },
        )
        .expect("projection");
        let prompt = materialize_oracle_prompt(&store, &projection).expect("materialize");
        let spec = prompt
            .native_spec(
                OracleAgentRole::Blue,
                ModelName::new("recorded-blue").expect("model"),
                ModelOutputTokenLimit::new(2048).expect("output limit"),
            )
            .expect("native spec");
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let initial = codec
            .prepare_initial(&spec, prompt.user_text())
            .expect("initial request");
        let initial_json: serde_json::Value =
            serde_json::from_slice(initial.request_bytes()).expect("initial JSON");
        assert!(initial_json["tools"].as_array().is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool["name"] == "oracle_search_external_tests")
        }));
        assert!(!initial_json["tools"].as_array().is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool["name"] == "oracle_submit_wrong_variant")
        }));

        let response = br#"{"output":[{"type":"function_call","call_id":"search-1","name":"oracle_search_external_tests","arguments":"{\"schema_version\":1,\"query\":\"reduction edge case\",\"repositories\":[\"pytorch/pytorch\"],\"max_results\":2}"}]}"#;
        let response_id = ContentId::derive(response).expect("response identity");
        let decoded = codec
            .decode_turn(&initial, response_id, response)
            .expect("decode search call");
        let settled = codec
            .append_tool_results(
                decoded.continuation(),
                &[NativeToolResult {
                    call_id: ProviderToolCallId::new("search-1").expect("call id"),
                    output: "{\"schema_version\":1,\"cases\":[]}".to_owned(),
                }],
            )
            .expect("settle tool result");
        let before_restart = codec
            .prepare_continuation(&spec, &settled)
            .expect("second turn");
        let continuation_id = codec.archive(&mut store, &settled).expect("archive");
        drop(store);

        let store = SqliteContentStore::open(&database, &cas).expect("reopen store");
        let recovered = codec.recover(&store, &continuation_id).expect("recover");
        let after_restart = codec
            .prepare_continuation(&spec, &recovered)
            .expect("recovered second turn");
        assert_eq!(
            before_restart.request_bytes(),
            after_restart.request_bytes()
        );
        let second_json: serde_json::Value =
            serde_json::from_slice(after_restart.request_bytes()).expect("second JSON");
        assert_eq!(initial_json["instructions"], second_json["instructions"]);
        assert_eq!(initial_json["tools"], second_json["tools"]);
        assert_eq!(initial_json["input"][0], second_json["input"][0]);
    }
}
