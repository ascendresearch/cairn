//! Trusted model-visible tools for blue proposal and red attack submission.

use cairn_agent::{
    CanonicalToolResult, NativeToolDefinition, PreparedToolOperation, ToolEffectClass, ToolGateway,
    ToolGatewayError, ToolImplementationVersion, ToolName, ToolRegistration,
};
use cairn_protocol::{ContentId, ContentType};
use cairn_verification::{
    AuthorshipOrigin, CorpusCaseArtifact, DomainRefinementArtifact, DomainRefinementV1,
    ImplementationVariantArtifact, ImplementationVariantV1, OracleProposalV1, VariantExpectation,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    ExternalTestSearchResultArtifact, OracleAgentRole, OracleAttackInput, OracleRoleTool,
    OracleSearchPlanError, OracleSearchPlanV1, PreparedOracleAttack,
    PreparedOracleProposalRevision, prepare_oracle_attack, prepare_oracle_proposal_revision,
};

const SCHEMA_V1: u16 = 1;
const TOOL_VERSION: &str = "oracle-workflow-v1";

const SEARCH: &str = "oracle_search_external_tests";
const SUBMIT_DOMAIN_REFINEMENT: &str = "oracle_submit_domain_refinement";
const SUBMIT_PROPOSAL: &str = "oracle_submit_oracle_proposal";
const SUBMIT_CORRECT: &str = "oracle_submit_correct_variant";
const SUBMIT_WRONG: &str = "oracle_submit_wrong_variant";
const SUBMIT_ADVERSARIAL: &str = "oracle_submit_adversarial_case";

/// Canonical serializable model-visible contract from which both catalog identity and wire tools
/// are derived.
#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OracleToolContractV1 {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
    strict: bool,
}

impl OracleToolContractV1 {
    fn native(&self) -> Result<NativeToolDefinition, OracleToolError> {
        Ok(NativeToolDefinition {
            name: ToolName::new(self.name).map_err(|_| OracleToolError::BuiltInContract)?,
            description: self.description.to_owned(),
            input_schema: self.input_schema.clone(),
            strict: self.strict,
        })
    }
}

pub(crate) fn oracle_role_tool_contracts(
    role: OracleAgentRole,
) -> Result<Vec<OracleToolContractV1>, OracleSearchPlanError> {
    role_tools(role).map_err(|error| OracleSearchPlanError::Encoding(error.to_string()))
}

/// Returns exact deterministic protocol-native definitions for one role.
///
/// # Errors
///
/// Returns an error only if a repository-owned name violates the generic tool-name contract.
pub fn oracle_role_native_tools(
    role: OracleAgentRole,
) -> Result<Vec<NativeToolDefinition>, OracleToolError> {
    role_tools(role)?
        .iter()
        .map(OracleToolContractV1::native)
        .collect()
}

fn role_tools(role: OracleAgentRole) -> Result<Vec<OracleToolContractV1>, OracleToolError> {
    let tools = match role {
        OracleAgentRole::Blue => vec![
            contract(
                OracleRoleTool::SearchExternalTests,
                "Search bounded operator-approved upstream repositories and fetch exact test bytes.",
                json!({
                    "type": "object",
                    "properties": {
                        "schema_version": {"type": "integer", "const": 1},
                        "query": {"type": "string", "minLength": 1, "maxLength": 256, "pattern": "^[A-Za-z0-9_. -]+$"},
                        "repositories": {"type": "array", "minItems": 1, "maxItems": 8, "uniqueItems": true, "items": {"type": "string", "pattern": "^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$"}},
                        "max_results": {"type": "integer", "minimum": 1, "maximum": 10}
                    },
                    "required": ["schema_version", "query", "repositories", "max_results"],
                    "additionalProperties": false
                }),
                true,
            ),
            contract(
                OracleRoleTool::SubmitDomainRefinement,
                "Submit one canonical domain-refinement JSON body; it remains a proposal delta.",
                canonical_json_string_schema("refinement_json"),
                true,
            ),
            contract(
                OracleRoleTool::SubmitOracleProposal,
                "Submit a complete typed OracleProposalV1 plus cited external research identities.",
                json!({
                    "type": "object",
                    "properties": {
                        "schema_version": {"type": "integer", "const": 1},
                        "proposal": {"type": "object"},
                        "external_research": {"type": "array", "uniqueItems": true, "items": {"type": "string"}}
                    },
                    "required": ["schema_version", "proposal", "external_research"],
                    "additionalProperties": false
                }),
                false,
            ),
        ],
        OracleAgentRole::Red => vec![
            contract(
                OracleRoleTool::SubmitCorrectVariant,
                "Submit one typed correct-by-construction implementation variant.",
                canonical_json_string_schema("variant_json"),
                true,
            ),
            contract(
                OracleRoleTool::SubmitWrongVariant,
                "Submit one typed deliberately wrong implementation variant.",
                canonical_json_string_schema("variant_json"),
                true,
            ),
            contract(
                OracleRoleTool::SubmitAdversarialCase,
                "Submit one canonical adversarial corpus-case identity.",
                canonical_json_string_schema("case_id"),
                true,
            ),
        ],
    };
    for tool in &tools {
        let _ = tool.native()?;
    }
    Ok(tools)
}

fn contract(
    tool: OracleRoleTool,
    description: &'static str,
    input_schema: Value,
    strict: bool,
) -> OracleToolContractV1 {
    OracleToolContractV1 {
        name: tool_name(tool),
        description,
        input_schema,
        strict,
    }
}

const fn tool_name(tool: OracleRoleTool) -> &'static str {
    match tool {
        OracleRoleTool::SearchExternalTests => SEARCH,
        OracleRoleTool::SubmitDomainRefinement => SUBMIT_DOMAIN_REFINEMENT,
        OracleRoleTool::SubmitOracleProposal => SUBMIT_PROPOSAL,
        OracleRoleTool::SubmitCorrectVariant => SUBMIT_CORRECT,
        OracleRoleTool::SubmitWrongVariant => SUBMIT_WRONG,
        OracleRoleTool::SubmitAdversarialCase => SUBMIT_ADVERSARIAL,
    }
}

fn canonical_json_string_schema(field: &'static str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "schema_version": {"type": "integer", "const": 1},
            (field): {"type": "string", "minLength": 2}
        },
        "required": ["schema_version", field],
        "additionalProperties": false
    })
}

/// Exact blue aggregate-submission arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlueProposalSubmissionV1 {
    schema_version: u16,
    /// Complete ordinary proposal body validated at the product boundary.
    pub proposal: OracleProposalV1,
    /// Exact external research results cited by this revision, in canonical wire order.
    pub external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BlueDomainRefinementSubmissionV1 {
    schema_version: u16,
    refinement_json: String,
}

/// Pure collector for independently model-authored Blue domain refinements.
pub struct BlueDomainRefinementGateway {
    plan: OracleSearchPlanV1,
    accepted: Vec<(ContentId<DomainRefinementArtifact>, DomainRefinementV1)>,
}

impl BlueDomainRefinementGateway {
    /// Creates an empty collector bound to one exact Blue episode and declared domain.
    #[must_use]
    pub const fn new(plan: OracleSearchPlanV1) -> Self {
        Self {
            plan,
            accepted: Vec::new(),
        }
    }

    /// Returns accepted refinement identities and validated bodies in submission order.
    #[must_use]
    pub fn accepted(&self) -> &[(ContentId<DomainRefinementArtifact>, DomainRefinementV1)] {
        &self.accepted
    }
}

impl ToolGateway for BlueDomainRefinementGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(operation, SUBMIT_DOMAIN_REFINEMENT)?;
        let input: BlueDomainRefinementSubmissionV1 = decode_canonical(operation.argument_bytes())?;
        if input.schema_version != SCHEMA_V1 {
            return rejected("unsupported Blue domain-refinement submission schema");
        }
        let refinement: DomainRefinementV1 =
            cairn_codec::from_slice(input.refinement_json.as_bytes())
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let canonical = cairn_codec::to_vec(&refinement)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        if canonical != input.refinement_json.as_bytes() {
            return rejected("Blue domain-refinement JSON is not canonical");
        }
        if refinement.declared_domain() != self.plan.declared_domain()
            || refinement.authorship().origin() != AuthorshipOrigin::Model
            || refinement.authorship().episode_id() != Some(self.plan.blue().episode_id())
            || refinement.authorship().model_configuration()
                != Some(self.plan.blue().authorship_configuration())
        {
            return rejected("Blue refinement domain, role, or model is inconsistent");
        }
        let id = content_id::<DomainRefinementArtifact>(&refinement)?;
        if self.accepted.iter().any(|(accepted, _)| *accepted == id) {
            return rejected("Blue domain refinement was already submitted");
        }
        self.accepted.push((id, refinement));
        accepted_identity(&id.to_wire())
    }
}

impl BlueProposalSubmissionV1 {
    /// Creates one current-V1 submission.
    #[must_use]
    pub const fn new(
        proposal: OracleProposalV1,
        external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_V1,
            proposal,
            external_research,
        }
    }
}

/// Pure gateway that validates blue model output and emits an immutable proposal revision.
pub struct BlueProposalGateway {
    plan: OracleSearchPlanV1,
    parent: Option<PreparedOracleProposalRevision>,
    accepted: Option<PreparedOracleProposalRevision>,
}

impl BlueProposalGateway {
    /// Creates a gateway for the first or next immutable blue revision.
    #[must_use]
    pub const fn new(
        plan: OracleSearchPlanV1,
        parent: Option<PreparedOracleProposalRevision>,
    ) -> Self {
        Self {
            plan,
            parent,
            accepted: None,
        }
    }

    /// Returns the last accepted typed revision.
    #[must_use]
    pub const fn accepted(&self) -> Option<&PreparedOracleProposalRevision> {
        self.accepted.as_ref()
    }
}

impl ToolGateway for BlueProposalGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(operation, SUBMIT_PROPOSAL)?;
        let input: BlueProposalSubmissionV1 = decode_canonical(operation.argument_bytes())?;
        if input.schema_version != SCHEMA_V1 {
            return Err(ToolGatewayError::Rejected(
                "unsupported blue proposal submission schema".to_owned(),
            ));
        }
        let prepared = prepare_oracle_proposal_revision(
            &self.plan,
            self.parent.as_ref(),
            input.proposal,
            input.external_research,
        )
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let result = CanonicalToolResult::from_value(
            &serde_json::to_value(prepared.body())
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?,
        )
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        self.accepted = Some(prepared);
        Ok(result)
    }
}

/// Exact canonical-JSON wrapper used by one red variant submission tool.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RedVariantSubmissionV1 {
    schema_version: u16,
    variant_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RedCaseSubmissionV1 {
    schema_version: u16,
    case_id: String,
}

/// Pure collector gateway for the three exact model-visible Red tools.
pub struct RedSubmissionGateway {
    plan: OracleSearchPlanV1,
    revision: PreparedOracleProposalRevision,
    correct_variants: Vec<ImplementationVariantV1>,
    wrong_variants: Vec<ImplementationVariantV1>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

impl RedSubmissionGateway {
    /// Creates an empty collector over one exact Blue revision.
    #[must_use]
    pub const fn new(plan: OracleSearchPlanV1, revision: PreparedOracleProposalRevision) -> Self {
        Self {
            plan,
            revision,
            correct_variants: Vec::new(),
            wrong_variants: Vec::new(),
            adversarial_cases: Vec::new(),
        }
    }

    /// Finalizes the collected Red submissions into one typed attack.
    ///
    /// # Errors
    ///
    /// Rejects missing classes, duplicate identities, or any wrong role/model authorship.
    pub fn finish(self) -> Result<PreparedOracleAttack, crate::OracleWorkflowError> {
        prepare_oracle_attack(
            &self.plan,
            &self.revision,
            OracleAttackInput {
                correct_variants: self.correct_variants,
                wrong_variants: self.wrong_variants,
                adversarial_cases: self.adversarial_cases,
            },
        )
    }
}

impl ToolGateway for RedSubmissionGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            SUBMIT_CORRECT => {
                validate_operation(operation, SUBMIT_CORRECT)?;
                let variant = decode_red_variant(operation.argument_bytes())?;
                validate_red_variant(&self.plan, &variant, true)?;
                let id = content_id::<ImplementationVariantArtifact>(&variant)?;
                self.correct_variants.push(variant);
                accepted_identity(&id.to_wire())
            }
            SUBMIT_WRONG => {
                validate_operation(operation, SUBMIT_WRONG)?;
                let variant = decode_red_variant(operation.argument_bytes())?;
                validate_red_variant(&self.plan, &variant, false)?;
                let id = content_id::<ImplementationVariantArtifact>(&variant)?;
                self.wrong_variants.push(variant);
                accepted_identity(&id.to_wire())
            }
            SUBMIT_ADVERSARIAL => {
                validate_operation(operation, SUBMIT_ADVERSARIAL)?;
                let input: RedCaseSubmissionV1 = decode_canonical(operation.argument_bytes())?;
                if input.schema_version != SCHEMA_V1 {
                    return rejected("unsupported Red case submission schema");
                }
                let case = serde_json::from_value::<ContentId<CorpusCaseArtifact>>(Value::String(
                    input.case_id,
                ))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.adversarial_cases.push(case);
                accepted_identity(&case.to_wire())
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is not a Red role submission".to_owned(),
            )),
        }
    }
}

/// Returns the trusted blue proposal registration.
///
/// # Errors
///
/// Returns an error only for invalid built-in names.
pub fn blue_proposal_registration() -> Result<ToolRegistration, OracleToolError> {
    registration(SUBMIT_PROPOSAL)
}

/// Returns the trusted Blue domain-refinement registration.
///
/// # Errors
///
/// Returns an error only for an invalid built-in name.
pub fn blue_domain_refinement_registration() -> Result<ToolRegistration, OracleToolError> {
    registration(SUBMIT_DOMAIN_REFINEMENT)
}

/// Returns the trusted registrations for the exact three model-visible Red tools.
///
/// # Errors
///
/// Returns an error only for invalid built-in names.
pub fn red_submission_registrations() -> Result<Vec<ToolRegistration>, OracleToolError> {
    [SUBMIT_CORRECT, SUBMIT_WRONG, SUBMIT_ADVERSARIAL]
        .into_iter()
        .map(registration)
        .collect()
}

fn decode_red_variant(bytes: &[u8]) -> Result<ImplementationVariantV1, ToolGatewayError> {
    let input: RedVariantSubmissionV1 = decode_canonical(bytes)?;
    if input.schema_version != SCHEMA_V1 {
        return rejected("unsupported Red variant submission schema");
    }
    let variant: ImplementationVariantV1 =
        cairn_codec::from_slice(input.variant_json.as_bytes())
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    let canonical = cairn_codec::to_vec(&variant)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    if canonical != input.variant_json.as_bytes() {
        return rejected("Red variant JSON is not canonical");
    }
    Ok(variant)
}

fn validate_red_variant(
    plan: &OracleSearchPlanV1,
    variant: &ImplementationVariantV1,
    correct: bool,
) -> Result<(), ToolGatewayError> {
    let correct_expectation =
        matches!(variant.expectation(), VariantExpectation::MustAccept { .. });
    if variant.authorship().origin() != AuthorshipOrigin::Model
        || variant.authorship().episode_id() != Some(plan.red().episode_id())
        || variant.authorship().model_configuration() != Some(plan.red().authorship_configuration())
        || correct_expectation != correct
    {
        return rejected("Red variant role, model, or expectation is inconsistent");
    }
    Ok(())
}

fn content_id<T: ContentType>(value: &impl Serialize) -> Result<ContentId<T>, ToolGatewayError> {
    let bytes = cairn_codec::to_vec(value)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    ContentId::derive(&bytes).map_err(|error| ToolGatewayError::Rejected(error.to_string()))
}

fn accepted_identity(identity: &str) -> Result<CanonicalToolResult, ToolGatewayError> {
    CanonicalToolResult::from_value(&json!({"schema_version": SCHEMA_V1, "accepted": identity}))
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))
}

fn rejected<T>(message: &str) -> Result<T, ToolGatewayError> {
    Err(ToolGatewayError::Rejected(message.to_owned()))
}

fn registration(name: &'static str) -> Result<ToolRegistration, OracleToolError> {
    Ok(ToolRegistration::new(
        ToolName::new(name).map_err(|_| OracleToolError::BuiltInContract)?,
        ToolImplementationVersion::new(TOOL_VERSION)
            .map_err(|_| OracleToolError::BuiltInContract)?,
        ToolEffectClass::Pure,
    ))
}

fn validate_operation(
    operation: &PreparedToolOperation,
    name: &'static str,
) -> Result<(), ToolGatewayError> {
    if operation.tool().as_str() != name
        || operation.implementation_version().as_str() != TOOL_VERSION
        || operation.effect() != ToolEffectClass::Pure
    {
        return Err(ToolGatewayError::NotStarted(
            "operation does not match the trusted Oracle Agent registration".to_owned(),
        ));
    }
    Ok(())
}

fn decode_canonical<T>(bytes: &[u8]) -> Result<T, ToolGatewayError>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let value: T = cairn_codec::from_slice(bytes)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    let canonical = cairn_codec::to_vec(&value)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    if canonical != bytes {
        return Err(ToolGatewayError::Rejected(
            "Oracle Agent submission is not canonical".to_owned(),
        ));
    }
    Ok(value)
}

/// Invalid trusted Oracle Agent tool construction.
#[derive(Debug, Error)]
pub enum OracleToolError {
    /// A repository-owned tool name or implementation label violates the generic boundary.
    #[error("invalid built-in Oracle Agent tool contract")]
    BuiltInContract,
}

#[cfg(test)]
mod tests {
    use cairn_agent::{
        ContextBlock, EpisodeBudget, InstructionBlock, ResolvedRuntimeModelArtifact,
        ToolGateway as _, prepare_tool_operation,
    };
    use cairn_protocol::{ContentId, ContentType, EpisodeId, OperationId, TaskId};
    use cairn_store_sqlite::SqliteContentStore;
    use cairn_verification::{
        AdmissionPolicyArtifact, ArtifactAuthorId, ArtifactAuthorshipV1, AuthorshipOrigin,
        DeclaredDomainArtifact, DomainDifferenceArtifact, DomainRefinementEvidenceArtifact,
        DomainRefinementV1, ModelConfigurationArtifact, OracleTaskInputArtifact,
    };

    use super::{BlueDomainRefinementGateway, blue_domain_refinement_registration};
    use crate::{
        OracleAgentRole, OracleRoleEpisodeInput, OracleSearchPlanInput, OracleSearchPlanV1,
        prepare_oracle_role_episode,
    };

    fn id<T: ContentType>(label: &str) -> ContentId<T> {
        ContentId::derive(label.as_bytes()).expect("content id")
    }

    fn plan() -> OracleSearchPlanV1 {
        let blue = prepare_oracle_role_episode(OracleRoleEpisodeInput {
            role: OracleAgentRole::Blue,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>("blue runtime"),
            authorship_configuration: id::<ModelConfigurationArtifact>("blue authorship"),
            role_instruction: id::<InstructionBlock>("blue role"),
            private_context: Vec::new(),
            budget: EpisodeBudget::default(),
        })
        .expect("blue role");
        let red = prepare_oracle_role_episode(OracleRoleEpisodeInput {
            role: OracleAgentRole::Red,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>("red runtime"),
            authorship_configuration: id::<ModelConfigurationArtifact>("red authorship"),
            role_instruction: id::<InstructionBlock>("red role"),
            private_context: Vec::new(),
            budget: EpisodeBudget::default(),
        })
        .expect("red role");
        OracleSearchPlanV1::new(OracleSearchPlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("admission policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller and source context")],
            blue,
            red,
        })
        .expect("plan")
    }

    fn refinement(plan: &OracleSearchPlanV1) -> DomainRefinementV1 {
        let authorship = ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Model,
            ArtifactAuthorId::new("recorded-blue").expect("author"),
            Some(plan.blue().episode_id()),
            Some(plan.blue().authorship_configuration()),
        )
        .expect("authorship");
        DomainRefinementV1::new(
            plan.declared_domain(),
            id::<DomainDifferenceArtifact>("empty reduction returns additive identity"),
            vec![id::<DomainRefinementEvidenceArtifact>(
                "recorded upstream research",
            )],
            authorship,
        )
        .expect("refinement")
    }

    #[test]
    fn advertised_blue_refinement_tool_has_an_executable_typed_gateway() {
        let plan = plan();
        let refinement = refinement(&plan);
        let refinement_json =
            String::from_utf8(cairn_codec::to_vec(&refinement).expect("refinement JSON"))
                .expect("UTF-8");
        let arguments = serde_json::json!({
            "schema_version": 1,
            "refinement_json": refinement_json,
        });
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let registration = blue_domain_refinement_registration().expect("registration");
        let operation = prepare_tool_operation(
            &mut content,
            OperationId::new(),
            registration.name().clone(),
            registration.implementation_version().clone(),
            registration.effect(),
            &arguments,
        )
        .expect("operation");
        let mut gateway = BlueDomainRefinementGateway::new(plan);
        gateway.invoke(&operation).expect("accepted refinement");
        assert_eq!(gateway.accepted().len(), 1);
        assert!(gateway.invoke(&operation).is_err());
    }
}
