//! Trusted model-visible tools for optional synthesis and adversarial debate strategies.

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
    ExternalTestSearchResultArtifact, OracleDebateAttackInput, OracleDebateStrategy,
    OracleDebateTool, OracleModelDebatePlanError, OracleModelDebatePlanV1,
    PreparedOracleDebateAttack, PreparedOracleDebateProposalRevision, prepare_oracle_debate_attack,
    prepare_oracle_debate_proposal_revision,
};

const SCHEMA_V1: u16 = 1;
const TOOL_VERSION: &str = "oracle-model-debate-v1";

const SEARCH: &str = "oracle_model_debate_external_tests";
const SUBMIT_DOMAIN_REFINEMENT: &str = "oracle_model_debate_submit_domain_refinement";
const SUBMIT_PROPOSAL: &str = "oracle_model_debate_submit_oracle_proposal";
const SUBMIT_CORRECT: &str = "oracle_model_debate_submit_correct_variant";
const SUBMIT_WRONG: &str = "oracle_model_debate_submit_wrong_variant";
const SUBMIT_ADVERSARIAL: &str = "oracle_model_debate_submit_adversarial_case";

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
    fn native(&self) -> Result<NativeToolDefinition, OracleDebateToolError> {
        Ok(NativeToolDefinition {
            name: ToolName::new(self.name).map_err(|_| OracleDebateToolError::BuiltInContract)?,
            description: self.description.to_owned(),
            input_schema: self.input_schema.clone(),
            strict: self.strict,
        })
    }
}

pub(crate) fn oracle_debate_tool_contracts(
    strategy: OracleDebateStrategy,
) -> Result<Vec<OracleToolContractV1>, OracleModelDebatePlanError> {
    strategy_tools(strategy)
        .map_err(|error| OracleModelDebatePlanError::Encoding(error.to_string()))
}

/// Returns exact deterministic protocol-native definitions for one strategy.
///
/// # Errors
///
/// Returns an error only if a repository-owned name violates the generic tool-name contract.
pub fn oracle_debate_native_tools(
    strategy: OracleDebateStrategy,
) -> Result<Vec<NativeToolDefinition>, OracleDebateToolError> {
    strategy_tools(strategy)?
        .iter()
        .map(OracleToolContractV1::native)
        .collect()
}

fn strategy_tools(
    strategy: OracleDebateStrategy,
) -> Result<Vec<OracleToolContractV1>, OracleDebateToolError> {
    let tools = match strategy {
        OracleDebateStrategy::Synthesis => vec![
            contract(
                OracleDebateTool::SearchExternalTests,
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
                OracleDebateTool::SubmitDomainRefinement,
                "Submit one canonical domain-refinement JSON body; it remains a proposal delta.",
                canonical_json_string_schema("refinement_json"),
                true,
            ),
            contract(
                OracleDebateTool::SubmitOracleProposal,
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
        OracleDebateStrategy::Adversarial => vec![
            contract(
                OracleDebateTool::SubmitCorrectVariant,
                "Submit one typed correct-by-construction implementation variant.",
                canonical_json_string_schema("variant_json"),
                true,
            ),
            contract(
                OracleDebateTool::SubmitWrongVariant,
                "Submit one typed deliberately wrong implementation variant.",
                canonical_json_string_schema("variant_json"),
                true,
            ),
            contract(
                OracleDebateTool::SubmitAdversarialCase,
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
    tool: OracleDebateTool,
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

const fn tool_name(tool: OracleDebateTool) -> &'static str {
    match tool {
        OracleDebateTool::SearchExternalTests => SEARCH,
        OracleDebateTool::SubmitDomainRefinement => SUBMIT_DOMAIN_REFINEMENT,
        OracleDebateTool::SubmitOracleProposal => SUBMIT_PROPOSAL,
        OracleDebateTool::SubmitCorrectVariant => SUBMIT_CORRECT,
        OracleDebateTool::SubmitWrongVariant => SUBMIT_WRONG,
        OracleDebateTool::SubmitAdversarialCase => SUBMIT_ADVERSARIAL,
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

/// Exact synthesis aggregate-submission arguments.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynthesisProposalSubmissionV1 {
    schema_version: u16,
    /// Complete ordinary proposal body validated at the product boundary.
    pub proposal: OracleProposalV1,
    /// Exact external research results cited by this revision, in canonical wire order.
    pub external_research: Vec<ContentId<ExternalTestSearchResultArtifact>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SynthesisDomainRefinementSubmissionV1 {
    schema_version: u16,
    refinement_json: String,
}

/// Pure collector for independently model-authored Synthesis domain refinements.
pub struct SynthesisDomainRefinementGateway {
    plan: OracleModelDebatePlanV1,
    accepted: Vec<(ContentId<DomainRefinementArtifact>, DomainRefinementV1)>,
}

impl SynthesisDomainRefinementGateway {
    /// Creates an empty collector bound to one exact Synthesis episode and declared domain.
    #[must_use]
    pub const fn new(plan: OracleModelDebatePlanV1) -> Self {
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

impl ToolGateway for SynthesisDomainRefinementGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(operation, SUBMIT_DOMAIN_REFINEMENT)?;
        let input: SynthesisDomainRefinementSubmissionV1 =
            decode_canonical(operation.argument_bytes())?;
        if input.schema_version != SCHEMA_V1 {
            return rejected("unsupported Synthesis domain-refinement submission schema");
        }
        let refinement: DomainRefinementV1 =
            cairn_codec::from_slice(input.refinement_json.as_bytes())
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        let canonical = cairn_codec::to_vec(&refinement)
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
        if canonical != input.refinement_json.as_bytes() {
            return rejected("Synthesis domain-refinement JSON is not canonical");
        }
        if refinement.declared_domain() != self.plan.declared_domain()
            || refinement.authorship().origin() != AuthorshipOrigin::Model
            || refinement.authorship().episode_id() != Some(self.plan.synthesis().episode_id())
            || refinement.authorship().model_configuration()
                != Some(self.plan.synthesis().authorship_configuration())
        {
            return rejected("Synthesis refinement domain, strategy, or model is inconsistent");
        }
        let id = content_id::<DomainRefinementArtifact>(&refinement)?;
        if self.accepted.iter().any(|(accepted, _)| *accepted == id) {
            return rejected("Synthesis domain refinement was already submitted");
        }
        self.accepted.push((id, refinement));
        accepted_identity(&id.to_wire())
    }
}

impl SynthesisProposalSubmissionV1 {
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

/// Pure gateway that validates synthesis model output and emits an immutable proposal revision.
pub struct SynthesisProposalGateway {
    plan: OracleModelDebatePlanV1,
    parent: Option<PreparedOracleDebateProposalRevision>,
    accepted: Option<PreparedOracleDebateProposalRevision>,
}

impl SynthesisProposalGateway {
    /// Creates a gateway for the first or next immutable synthesis revision.
    #[must_use]
    pub const fn new(
        plan: OracleModelDebatePlanV1,
        parent: Option<PreparedOracleDebateProposalRevision>,
    ) -> Self {
        Self {
            plan,
            parent,
            accepted: None,
        }
    }

    /// Returns the last accepted typed revision.
    #[must_use]
    pub const fn accepted(&self) -> Option<&PreparedOracleDebateProposalRevision> {
        self.accepted.as_ref()
    }
}

impl ToolGateway for SynthesisProposalGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        validate_operation(operation, SUBMIT_PROPOSAL)?;
        let input: SynthesisProposalSubmissionV1 = decode_canonical(operation.argument_bytes())?;
        if input.schema_version != SCHEMA_V1 {
            return Err(ToolGatewayError::Rejected(
                "unsupported synthesis proposal submission schema".to_owned(),
            ));
        }
        let prepared = prepare_oracle_debate_proposal_revision(
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

/// Exact canonical-JSON wrapper used by one adversarial variant submission tool.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AdversarialVariantSubmissionV1 {
    schema_version: u16,
    variant_json: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AdversarialCaseSubmissionV1 {
    schema_version: u16,
    case_id: String,
}

/// Pure collector gateway for the three exact model-visible Adversarial tools.
pub struct AdversarialSubmissionGateway {
    plan: OracleModelDebatePlanV1,
    revision: PreparedOracleDebateProposalRevision,
    correct_variants: Vec<ImplementationVariantV1>,
    wrong_variants: Vec<ImplementationVariantV1>,
    adversarial_cases: Vec<ContentId<CorpusCaseArtifact>>,
}

impl AdversarialSubmissionGateway {
    /// Creates an empty collector over one exact Synthesis revision.
    #[must_use]
    pub const fn new(
        plan: OracleModelDebatePlanV1,
        revision: PreparedOracleDebateProposalRevision,
    ) -> Self {
        Self {
            plan,
            revision,
            correct_variants: Vec::new(),
            wrong_variants: Vec::new(),
            adversarial_cases: Vec::new(),
        }
    }

    /// Finalizes the collected Adversarial submissions into one typed attack.
    ///
    /// # Errors
    ///
    /// Rejects missing classes, duplicate identities, or any wrong strategy/model authorship.
    pub fn finish(self) -> Result<PreparedOracleDebateAttack, crate::OracleDebateWorkflowError> {
        prepare_oracle_debate_attack(
            &self.plan,
            &self.revision,
            OracleDebateAttackInput {
                correct_variants: self.correct_variants,
                wrong_variants: self.wrong_variants,
                adversarial_cases: self.adversarial_cases,
            },
        )
    }
}

impl ToolGateway for AdversarialSubmissionGateway {
    fn invoke(
        &mut self,
        operation: &PreparedToolOperation,
    ) -> Result<CanonicalToolResult, ToolGatewayError> {
        match operation.tool().as_str() {
            SUBMIT_CORRECT => {
                validate_operation(operation, SUBMIT_CORRECT)?;
                let variant = decode_adversarial_variant(operation.argument_bytes())?;
                validate_adversarial_variant(&self.plan, &variant, true)?;
                let id = content_id::<ImplementationVariantArtifact>(&variant)?;
                self.correct_variants.push(variant);
                accepted_identity(&id.to_wire())
            }
            SUBMIT_WRONG => {
                validate_operation(operation, SUBMIT_WRONG)?;
                let variant = decode_adversarial_variant(operation.argument_bytes())?;
                validate_adversarial_variant(&self.plan, &variant, false)?;
                let id = content_id::<ImplementationVariantArtifact>(&variant)?;
                self.wrong_variants.push(variant);
                accepted_identity(&id.to_wire())
            }
            SUBMIT_ADVERSARIAL => {
                validate_operation(operation, SUBMIT_ADVERSARIAL)?;
                let input: AdversarialCaseSubmissionV1 =
                    decode_canonical(operation.argument_bytes())?;
                if input.schema_version != SCHEMA_V1 {
                    return rejected("unsupported Adversarial case submission schema");
                }
                let case = serde_json::from_value::<ContentId<CorpusCaseArtifact>>(Value::String(
                    input.case_id,
                ))
                .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
                self.adversarial_cases.push(case);
                accepted_identity(&case.to_wire())
            }
            _ => Err(ToolGatewayError::NotStarted(
                "operation is not a Adversarial strategy submission".to_owned(),
            )),
        }
    }
}

/// Returns the trusted synthesis proposal registration.
///
/// # Errors
///
/// Returns an error only for invalid built-in names.
pub fn synthesis_proposal_registration() -> Result<ToolRegistration, OracleDebateToolError> {
    registration(SUBMIT_PROPOSAL)
}

/// Returns the trusted Synthesis domain-refinement registration.
///
/// # Errors
///
/// Returns an error only for an invalid built-in name.
pub fn synthesis_domain_refinement_registration() -> Result<ToolRegistration, OracleDebateToolError>
{
    registration(SUBMIT_DOMAIN_REFINEMENT)
}

/// Returns the trusted registrations for the exact three model-visible Adversarial tools.
///
/// # Errors
///
/// Returns an error only for invalid built-in names.
pub fn adversarial_submission_registrations() -> Result<Vec<ToolRegistration>, OracleDebateToolError>
{
    [SUBMIT_CORRECT, SUBMIT_WRONG, SUBMIT_ADVERSARIAL]
        .into_iter()
        .map(registration)
        .collect()
}

fn decode_adversarial_variant(bytes: &[u8]) -> Result<ImplementationVariantV1, ToolGatewayError> {
    let input: AdversarialVariantSubmissionV1 = decode_canonical(bytes)?;
    if input.schema_version != SCHEMA_V1 {
        return rejected("unsupported Adversarial variant submission schema");
    }
    let variant: ImplementationVariantV1 =
        cairn_codec::from_slice(input.variant_json.as_bytes())
            .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    let canonical = cairn_codec::to_vec(&variant)
        .map_err(|error| ToolGatewayError::Rejected(error.to_string()))?;
    if canonical != input.variant_json.as_bytes() {
        return rejected("Adversarial variant JSON is not canonical");
    }
    Ok(variant)
}

fn validate_adversarial_variant(
    plan: &OracleModelDebatePlanV1,
    variant: &ImplementationVariantV1,
    correct: bool,
) -> Result<(), ToolGatewayError> {
    let correct_expectation =
        matches!(variant.expectation(), VariantExpectation::MustAccept { .. });
    if variant.authorship().origin() != AuthorshipOrigin::Model
        || variant.authorship().episode_id() != Some(plan.adversarial().episode_id())
        || variant.authorship().model_configuration()
            != Some(plan.adversarial().authorship_configuration())
        || correct_expectation != correct
    {
        return rejected("Adversarial variant strategy, model, or expectation is inconsistent");
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

fn registration(name: &'static str) -> Result<ToolRegistration, OracleDebateToolError> {
    Ok(ToolRegistration::new(
        ToolName::new(name).map_err(|_| OracleDebateToolError::BuiltInContract)?,
        ToolImplementationVersion::new(TOOL_VERSION)
            .map_err(|_| OracleDebateToolError::BuiltInContract)?,
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
pub enum OracleDebateToolError {
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

    use super::{SynthesisDomainRefinementGateway, synthesis_domain_refinement_registration};
    use crate::{
        OracleDebateEpisodeInput, OracleDebateStrategy, OracleModelDebatePlanInput,
        OracleModelDebatePlanV1, prepare_oracle_debate_episode,
    };

    fn id<T: ContentType>(label: &str) -> ContentId<T> {
        ContentId::derive(label.as_bytes()).expect("content id")
    }

    fn plan() -> OracleModelDebatePlanV1 {
        let synthesis = prepare_oracle_debate_episode(OracleDebateEpisodeInput {
            strategy: OracleDebateStrategy::Synthesis,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>("synthesis runtime"),
            authorship_configuration: id::<ModelConfigurationArtifact>("synthesis authorship"),
            strategy_instruction: id::<InstructionBlock>("synthesis strategy"),
            private_context: Vec::new(),
            budget: EpisodeBudget::default(),
        })
        .expect("synthesis strategy");
        let adversarial = prepare_oracle_debate_episode(OracleDebateEpisodeInput {
            strategy: OracleDebateStrategy::Adversarial,
            episode_id: EpisodeId::new(),
            model_configuration: id::<ResolvedRuntimeModelArtifact>("adversarial runtime"),
            authorship_configuration: id::<ModelConfigurationArtifact>("adversarial authorship"),
            strategy_instruction: id::<InstructionBlock>("adversarial strategy"),
            private_context: Vec::new(),
            budget: EpisodeBudget::default(),
        })
        .expect("adversarial strategy");
        OracleModelDebatePlanV1::new(OracleModelDebatePlanInput {
            task_id: TaskId::new(),
            task_inputs: id::<OracleTaskInputArtifact>("task inputs"),
            declared_domain: id::<DeclaredDomainArtifact>("declared domain"),
            admission_policy: id::<AdmissionPolicyArtifact>("admission policy"),
            common_instructions: vec![id::<InstructionBlock>("common")],
            shared_context: vec![id::<ContextBlock>("caller and source context")],
            synthesis,
            adversarial,
        })
        .expect("plan")
    }

    fn refinement(plan: &OracleModelDebatePlanV1) -> DomainRefinementV1 {
        let authorship = ArtifactAuthorshipV1::new(
            AuthorshipOrigin::Model,
            ArtifactAuthorId::new("recorded-synthesis").expect("author"),
            Some(plan.synthesis().episode_id()),
            Some(plan.synthesis().authorship_configuration()),
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
    fn advertised_synthesis_refinement_tool_has_an_executable_typed_gateway() {
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
        let registration = synthesis_domain_refinement_registration().expect("registration");
        let operation = prepare_tool_operation(
            &mut content,
            OperationId::new(),
            registration.name().clone(),
            registration.implementation_version().clone(),
            registration.effect(),
            &arguments,
        )
        .expect("operation");
        let mut gateway = SynthesisDomainRefinementGateway::new(plan);
        gateway.invoke(&operation).expect("accepted refinement");
        assert_eq!(gateway.accepted().len(), 1);
        assert!(gateway.invoke(&operation).is_err());
    }
}
