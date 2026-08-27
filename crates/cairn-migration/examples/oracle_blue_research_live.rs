//! Opt-in Blue Oracle dogfood: real model, recorded upstream research, durable restart.

use std::{
    io::Cursor,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use cairn_agent::{
    AdapterVersion, ContextBlock, DeploymentName, DispatchCompletion, EpisodeBudget,
    EpisodeProviderTokenLimit, EpisodeStepLimit, EpisodeToolOperationLimit, HistoryItem,
    HttpModelTransport, ModelName, ModelOutputTokenLimit, ModelProtocolKind, ModelSelection,
    ModelTemplate, ModelTemplateRegistry, ModelTransport, NativeContinuation, NativeProtocolCodec,
    NativeRequestSpec, NativeToolResult, OperationResult, PolicyDocument, PreparedNativeRequest,
    ProviderName, ReceivedModelResponse, ResolvedRuntimeModelArtifact, RuntimeModelCatalog,
    SemanticModelTurn, SemanticOutputItem, ToolCatalog, TurnInputDecision, authorize_model_request,
    authorize_tool_operation, begin_model_dispatch, begin_tool_operation, execute_model_dispatch,
    execute_tool_operation, prepare_native_dispatch_request, prepare_tool_operation,
};
use cairn_migration::{
    ExternalResearchPolicy, ExternalResearchProvider, ExternalResearchProviderError,
    ExternalTestCaseV1, ExternalTestResearchContextV1, ExternalTestSearchGateway,
    ExternalTestSearchRequestArtifact, ExternalTestSearchRequestV1, ExternalTestSearchResultV1,
    GitHubBlobIdentity, GitHubExternalResearchProvider, GitHubRepository, OracleAgentRole,
    OracleRoleEpisodeInput, OracleRolePromptInput, OracleSearchPlanInput, OracleSearchPlanV1,
    RecordedExternalResearchExchange, RecordedExternalResearchProvider, SearchResultLimit,
    SourcePath, archive_external_test_evidence, archive_oracle_role_tool_catalog,
    archive_standard_oracle_instructions, external_test_search_registration,
    materialize_oracle_prompt, prepare_oracle_role_episode, prepare_oracle_role_prompt,
};
use cairn_protocol::{
    AggregateId, AggregateKind, AttemptId, CommandId, ContentId, ContentType, EpisodeId,
    ModelAttemptId, ObservedAtUnixMillis, OperationId, TaskId,
};
use cairn_record::{ContentStore, ExpectedRevision, StreamId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use cairn_verification::{
    AdmissionPolicyArtifact, DeclaredDomainArtifact, ModelConfigurationArtifact,
    OracleTaskInputArtifact,
};
use serde::{Deserialize, Serialize};

enum BlueDogfoodDraftArtifact {}

impl ContentType for BlueDogfoodDraftArtifact {
    const DOMAIN: &'static str = "migration.blue-dogfood-draft.v1";
}

enum RedDogfoodReviewArtifact {}

impl ContentType for RedDogfoodReviewArtifact {
    const DOMAIN: &'static str = "migration.red-dogfood-review.v1";
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BlueDogfoodDraftV1 {
    schema_version: u16,
    case_name: String,
    input: String,
    invocation: String,
    expectation: DraftExpectationV1,
    comparison: DraftComparisonV1,
    rationale: String,
    evidence_assessment: String,
    assumptions: Vec<String>,
    unverified: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum DraftExpectationV1 {
    Exact { output: String },
    Property { predicate: String },
    Reject { error_behavior: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum DraftComparisonV1 {
    Exact,
    Numeric {
        absolute_tolerance: String,
        relative_tolerance: String,
        ulp_tolerance: u64,
        nan_equal: bool,
    },
    Property,
    NotApplicable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RedDogfoodReviewV1 {
    schema_version: u16,
    verdict: RedReviewVerdict,
    strengths: Vec<String>,
    blocking_findings: Vec<RedReviewFindingV1>,
    advisories: Vec<RedReviewFindingV1>,
    recommended_revision: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RedReviewVerdict {
    Pass,
    Revise,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RedReviewFindingV1 {
    kind: RedReviewFindingKind,
    detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RedReviewFindingKind {
    FalseAccept,
    FalseReject,
    Underspecified,
}

impl RedDogfoodReviewV1 {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "schema_version must be 1, received {}",
                self.schema_version
            ));
        }
        if self.recommended_revision.trim().is_empty() {
            return Err("recommended_revision must be a nonempty string".to_owned());
        }
        if self.strengths.len() > 16 {
            return Err("strengths exceeds the maximum of 16 entries".to_owned());
        }
        if let Some(index) = self
            .strengths
            .iter()
            .position(|item| item.trim().is_empty())
        {
            return Err(format!("strengths[{index}] must be nonempty"));
        }
        for (field, findings) in [
            ("blocking_findings", &self.blocking_findings),
            ("advisories", &self.advisories),
        ] {
            if findings.len() > 16 {
                return Err(format!("{field} exceeds the maximum of 16 entries"));
            }
            if let Some(index) = findings
                .iter()
                .position(|finding| finding.detail.trim().is_empty())
            {
                return Err(format!("{field}[{index}].detail must be nonempty"));
            }
        }
        if matches!(self.verdict, RedReviewVerdict::Pass) != self.blocking_findings.is_empty() {
            return Err(format!(
                "verdict/blocker mismatch: verdict is {:?} but blocking_findings has {} entries; pass requires zero blockers and revise requires at least one",
                self.verdict,
                self.blocking_findings.len()
            ));
        }
        Ok(())
    }
}

impl BlueDogfoodDraftV1 {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "schema_version must be 1, received {}",
                self.schema_version
            ));
        }
        let required = [
            ("case_name", self.case_name.as_str()),
            ("input", self.input.as_str()),
            ("invocation", self.invocation.as_str()),
            ("rationale", self.rationale.as_str()),
            ("evidence_assessment", self.evidence_assessment.as_str()),
        ];
        for (field, value) in required {
            if value.trim().is_empty() {
                return Err(format!("{field} must be a nonempty string"));
            }
            if value.chars().any(char::is_control) {
                return Err(format!("{field} contains a control character"));
            }
        }
        for (field, values) in [
            ("assumptions", &self.assumptions),
            ("unverified", &self.unverified),
        ] {
            if values.len() > 16 {
                return Err(format!("{field} exceeds the maximum of 16 entries"));
            }
            if let Some(index) = values.iter().position(|value| value.trim().is_empty()) {
                return Err(format!("{field}[{index}] must be nonempty"));
            }
        }
        match &self.expectation {
            DraftExpectationV1::Exact { output } if output.trim().is_empty() => {
                return Err("expectation.output must be nonempty for kind exact".to_owned());
            }
            DraftExpectationV1::Property { predicate } if predicate.trim().is_empty() => {
                return Err("expectation.predicate must be nonempty for kind property".to_owned());
            }
            DraftExpectationV1::Reject { error_behavior } if error_behavior.trim().is_empty() => {
                return Err(
                    "expectation.error_behavior must be nonempty for kind reject".to_owned(),
                );
            }
            _ => {}
        }
        let coherent = matches!(
            (&self.expectation, &self.comparison),
            (
                DraftExpectationV1::Exact { .. },
                DraftComparisonV1::Exact | DraftComparisonV1::Numeric { .. }
            ) | (
                DraftExpectationV1::Property { .. },
                DraftComparisonV1::Property
            ) | (
                DraftExpectationV1::Reject { .. },
                DraftComparisonV1::NotApplicable
            )
        );
        if !coherent {
            return Err(format!(
                "expectation/comparison mismatch: expectation is {:?} and comparison is {:?}",
                self.expectation, self.comparison
            ));
        }
        if let DraftComparisonV1::Numeric {
            absolute_tolerance,
            relative_tolerance,
            ..
        } = &self.comparison
        {
            for (field, value) in [
                ("absolute_tolerance", absolute_tolerance),
                ("relative_tolerance", relative_tolerance),
            ] {
                let parsed = value
                    .parse::<f64>()
                    .map_err(|_| format!("comparison.{field} must be a decimal string"))?;
                if !parsed.is_finite() || parsed.is_sign_negative() {
                    return Err(format!("comparison.{field} must be finite and nonnegative"));
                }
            }
        }
        Ok(())
    }
}

struct DogfoodSample {
    name: &'static str,
    operator: &'static str,
    query: &'static str,
    caller: serde_json::Value,
    source: serde_json::Value,
    task: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveConfig {
    schema_version: u16,
    blue: RoleLimits,
    red: RoleLimits,
    workflow: WorkflowLimits,
    research: ResearchConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkflowLimits {
    #[serde(rename = "max_blue_submission_repairs")]
    blue_submission_repairs: u32,
    #[serde(rename = "max_red_submission_repairs")]
    red_submission_repairs: u32,
    #[serde(rename = "max_adversarial_rounds")]
    adversarial_rounds: u32,
    #[serde(rename = "max_stability_rechecks")]
    stability_rechecks: u32,
}

impl WorkflowLimits {
    fn validate(&self) -> Result<(), &'static str> {
        if self.blue_submission_repairs == 0
            || self.red_submission_repairs == 0
            || self.adversarial_rounds == 0
            || self.stability_rechecks == 0
        {
            return Err("all Oracle dogfood workflow limits must be positive");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleLimits {
    #[serde(rename = "max_turns")]
    turns: u32,
    #[serde(rename = "max_tool_operations")]
    tool_operations: u32,
    #[serde(rename = "max_provider_tokens")]
    provider_tokens: u64,
    #[serde(rename = "max_output_tokens_per_turn")]
    output_tokens_per_turn: u64,
}

impl RoleLimits {
    fn budget(&self) -> Result<EpisodeBudget, Box<dyn std::error::Error>> {
        Ok(EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(self.turns)?),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(self.tool_operations)),
            provider_token_limit: Some(EpisodeProviderTokenLimit::new(self.provider_tokens)?),
            deadline_unix_ms: None,
            external_meter_limits: None,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchConfig {
    provider: ResearchProviderConfig,
    repositories: Vec<String>,
    max_results_per_search: u16,
    max_response_bytes: u64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum ResearchProviderConfig {
    Recorded,
    Github { credential_file: PathBuf },
}

enum ConfiguredResearchProvider {
    Recorded(RecordedExternalResearchProvider),
    Github(GitHubExternalResearchProvider),
}

impl ExternalResearchProvider for ConfiguredResearchProvider {
    fn search(
        &mut self,
        request: &ExternalTestSearchRequestV1,
    ) -> Result<ExternalTestSearchResultV1, ExternalResearchProviderError> {
        match self {
            Self::Recorded(provider) => provider.search(request),
            Self::Github(provider) => provider.search(request),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cairn_observability::init("oracle-blue-research-live")?;
    let root = std::env::current_dir()?;
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/oracle-blue-dogfood.example.json".to_owned());
    let sample_name = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "sum-empty-axis".to_owned());
    let sample = dogfood_sample(&sample_name)?;
    tracing::info!(
        target: "cairn.oracle.dogfood",
        event = "oracle_dogfood_started",
        sample = sample.name,
        operator = sample.operator,
        "oracle dogfood run started"
    );
    let live: LiveConfig = serde_json::from_slice(&std::fs::read(root.join(config_path))?)?;
    if live.schema_version != 1 || live.research.repositories.is_empty() {
        return Err("unsupported live dogfood configuration".into());
    }
    live.workflow.validate()?;
    let repositories = live
        .research
        .repositories
        .iter()
        .map(GitHubRepository::new)
        .collect::<Result<Vec<_>, _>>()?;
    if repositories.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("research repositories must be unique and sorted".into());
    }
    if live.research.max_response_bytes == 0 {
        return Err("research response-byte limit must be positive".into());
    }
    let search_limit = SearchResultLimit::new(live.research.max_results_per_search)?;
    let template: ModelTemplate = serde_json::from_slice(&std::fs::read(
        root.join("model-templates/deepseek/deepseek-v4-pro.json"),
    )?)?;
    let templates = ModelTemplateRegistry::from_templates([template])?;
    let catalog: RuntimeModelCatalog = serde_json::from_slice(&std::fs::read(
        root.join("config/runtime-models.example.json"),
    )?)?;
    let model = catalog.resolve(&templates, None)?;
    if model.protocol().kind() != ModelProtocolKind::OpenAiResponses {
        return Err("Blue dogfood requires the Responses deployment".into());
    }

    let directory = tempfile::tempdir()?;
    let content_database = directory.path().join("content.db");
    let event_database = directory.path().join("events.db");
    let cas = directory.path().join("cas");
    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    archive_oracle_role_tool_catalog(&mut content, OracleAgentRole::Blue)?;
    archive_oracle_role_tool_catalog(&mut content, OracleAgentRole::Red)?;

    let blue_instructions =
        archive_standard_oracle_instructions(&mut content, OracleAgentRole::Blue)?;
    let red_instructions =
        archive_standard_oracle_instructions(&mut content, OracleAgentRole::Red)?;
    let common = blue_instructions.common();
    let blue_instruction = blue_instructions.role();
    let red_instruction = red_instructions.role();
    let caller = put_json::<ContextBlock>(&mut content, &sample.caller)?;
    let source = put_json::<ContextBlock>(&mut content, &sample.source)?;
    let repository_request = serde_json::to_string(&live.research.repositories)?;
    let request = put_json::<HistoryItem>(
        &mut content,
        &serde_json::json!({
            "role":"user",
            "content": format!(
                "For sample '{}', first call oracle_search_external_tests with schema_version 1, query '{}', repositories {repository_request}, and max_results {}. Then use the result only as research and independently produce this oracle draft: {}. This dogfood stage expects final JSON rather than a production submission tool. {}",
                sample.name,
                sample.query,
                live.research.max_results_per_search
                ,sample.task,
                blue_dogfood_contract()
            )
        }),
    )?;
    let policy_document = put_json::<PolicyDocument>(
        &mut content,
        &serde_json::json!({"network":"configured-upstream-only","repositories":live.research.repositories}),
    )?;
    let runtime_id =
        ContentId::<ResolvedRuntimeModelArtifact>::derive(&cairn_codec::to_vec(&model)?)?;
    let blue = prepare_oracle_role_episode(OracleRoleEpisodeInput {
        role: OracleAgentRole::Blue,
        episode_id: EpisodeId::new(),
        model_configuration: runtime_id,
        authorship_configuration: ContentId::<ModelConfigurationArtifact>::derive(
            b"live-blue-dogfood",
        )?,
        role_instruction: blue_instruction,
        private_context: Vec::new(),
        budget: live.blue.budget()?,
    })?;
    let red = prepare_oracle_role_episode(OracleRoleEpisodeInput {
        role: OracleAgentRole::Red,
        episode_id: EpisodeId::new(),
        model_configuration: runtime_id,
        authorship_configuration: ContentId::<ModelConfigurationArtifact>::derive(
            b"unused-red-dogfood",
        )?,
        role_instruction: red_instruction,
        private_context: Vec::new(),
        budget: live.red.budget()?,
    })?;
    let plan = OracleSearchPlanV1::new(OracleSearchPlanInput {
        task_id: TaskId::new(),
        task_inputs: ContentId::<OracleTaskInputArtifact>::derive(sample.name.as_bytes())?,
        declared_domain: ContentId::<DeclaredDomainArtifact>::derive(sample.operator.as_bytes())?,
        admission_policy: ContentId::<AdmissionPolicyArtifact>::derive(b"blue dogfood policy v1")?,
        common_instructions: vec![common],
        shared_context: vec![caller, source],
        blue,
        red,
    })?;
    let projection = prepare_oracle_role_prompt(
        &plan,
        OracleRolePromptInput {
            role: OracleAgentRole::Blue,
            append_only_context: Vec::new(),
            diagnostic_context: Vec::new(),
            current_request: request,
            policy: policy_document,
        },
    )?;
    let prompt = materialize_oracle_prompt(&content, &projection)?;
    let output_limit = ModelOutputTokenLimit::new(live.blue.output_tokens_per_turn)?;
    if output_limit > model.capabilities().max_output_tokens()
        || ModelOutputTokenLimit::new(live.red.output_tokens_per_turn)?
            > model.capabilities().max_output_tokens()
    {
        return Err("role output limit exceeds the model template".into());
    }
    let spec = prompt.native_spec(
        OracleAgentRole::Blue,
        ModelName::new(model.wire_model().as_str())?,
        output_limit,
    )?;
    let codec = NativeProtocolCodec::from_config(model.protocol())?;
    let initial = codec.prepare_initial(&spec, prompt.user_text())?;
    let mut transport = HttpModelTransport::new(&model, &root)?;
    let first_attempt = ModelAttemptId::new();
    let first_stream = stream(first_attempt)?;
    let first_decision = projection.turn_input_decision(selection(&model)?);
    let first_received = dispatch(
        &mut events,
        &mut content,
        &mut transport,
        &first_stream,
        first_attempt,
        &first_decision,
        &initial,
    )?;
    let first_usage = first_received.usage();
    let decoded = codec.decode_recovered_received(
        &mut events,
        &mut content,
        first_received,
        &CommandId::new(),
        now()?,
    )?;
    if decoded.semantic().proposals().len() != 1
        || decoded.semantic().proposals()[0].tool().as_str() != "oracle_search_external_tests"
    {
        return Err("Blue did not issue exactly one bounded external-test search".into());
    }
    let proposal = &decoded.semantic().proposals()[0];
    let mut argument_bytes = Vec::new();
    content.write_to(&proposal.arguments_id(), &mut argument_bytes)?;
    let search_request: ExternalTestSearchRequestV1 = cairn_codec::from_slice(&argument_bytes)?;
    let search_request_id = ContentId::<ExternalTestSearchRequestArtifact>::derive(
        &cairn_codec::to_vec(&search_request)?,
    )?;
    let research_case = ExternalTestCaseV1::new(
        repositories[0].clone(),
        SourcePath::new("test/test_reductions.py")?,
        GitHubBlobIdentity::new("0123456789abcdef0123456789abcdef01234567")?,
        "def test_sum_empty(self):\n    assert torch.empty(0).sum() == 0\n".to_owned(),
    )?;
    let research_result = ExternalTestSearchResultV1::new(
        &search_request,
        "recorded-pytorch-dogfood".to_owned(),
        now()?,
        vec![research_case],
        0,
    )?;
    let provider = match live.research.provider {
        ResearchProviderConfig::Recorded => {
            ConfiguredResearchProvider::Recorded(RecordedExternalResearchProvider::new([
                RecordedExternalResearchExchange {
                    request: search_request_id,
                    result: research_result,
                },
            ]))
        }
        ResearchProviderConfig::Github { credential_file } => {
            ConfiguredResearchProvider::Github(GitHubExternalResearchProvider::new(
                Some(root.join(credential_file)),
                live.research.max_response_bytes,
            )?)
        }
    };
    let research_policy = ExternalResearchPolicy::new(repositories, search_limit)?;
    let registration = external_test_search_registration()?;
    let arguments: serde_json::Value = cairn_codec::from_slice(&argument_bytes)?;
    let operation = prepare_tool_operation(
        &mut content,
        OperationId::new(),
        registration.name().clone(),
        registration.implementation_version().clone(),
        registration.effect(),
        &arguments,
    )?;
    let authority = authorize_tool_operation(&mut events, &CommandId::new(), now()?, operation)?;
    let started = begin_tool_operation(
        &mut events,
        authority,
        AttemptId::new(),
        &CommandId::new(),
        now()?,
    )?;
    let mut gateway = ExternalTestSearchGateway::new(research_policy, provider);
    let completion = execute_tool_operation(
        &mut events,
        &mut content,
        &mut gateway,
        started,
        &CommandId::new(),
        now()?,
    )?;
    let cairn_agent::ToolOperationCompletion::Completed { result_id, .. } = completion else {
        return Err("recorded Blue research did not complete".into());
    };
    tracing::info!(
        target: "cairn.oracle.dogfood",
        event = "blue_research_completed",
        sample = sample.name,
        research_request_id = %search_request_id,
        operation_result_id = %result_id,
        "Blue external research completed"
    );
    let exact_research = gateway
        .result()
        .ok_or("research gateway completed without an exact result")?
        .clone();
    let _evidence = archive_external_test_evidence(&mut content, &search_request, &exact_research)?;
    let mut result_bytes = Vec::new();
    content.write_to(&result_id, &mut result_bytes)?;
    let research_context: ExternalTestResearchContextV1 = cairn_codec::from_slice(&result_bytes)?;
    let result_text = String::from_utf8(result_bytes)?;
    let call_id = decoded
        .continuation()
        .pending_call_ids()
        .first()
        .ok_or("Blue continuation lost the research call")?
        .clone();
    let settled = codec.append_tool_results(
        decoded.continuation(),
        &[NativeToolResult {
            call_id,
            output: result_text,
        }],
    )?;
    let before_restart = codec.prepare_continuation(&spec, &settled)?;
    let continuation_id = codec.archive(&mut content, &settled)?;
    drop(decoded);
    drop(events);
    drop(content);

    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    let recovered = codec.recover(&content, &continuation_id)?;
    let after_restart = codec.prepare_continuation(&spec, &recovered)?;
    if before_restart.request_bytes() != after_restart.request_bytes() {
        return Err("restart changed the Blue continuation request".into());
    }
    let second_decision = decision_after_research(
        &mut content,
        &model,
        result_id,
        policy_document,
        plan.blue().tool_catalog(),
    )?;
    let blue_turn = JsonTurnRuntime {
        events: &mut events,
        content: &mut content,
        transport: &mut transport,
        codec,
        spec: &spec,
        decision: &second_decision,
    }
    .run(
        after_restart,
        live.workflow.blue_submission_repairs,
        "Blue draft",
        blue_dogfood_contract(),
        BlueDogfoodDraftV1::validate,
    )?;
    let mut blue_usage = blue_turn.usage;
    let mut blue_repairs = blue_turn.repairs;
    let mut blue_continuation = blue_turn.continuation;
    let mut draft = blue_turn.value;
    let draft_bytes = cairn_codec::to_vec(&draft)?;
    let draft_descriptor =
        content.put::<BlueDogfoodDraftArtifact>(&mut Cursor::new(draft_bytes))?;
    let mut draft_ids = vec![draft_descriptor.content_id];
    let red_request = put_json::<HistoryItem>(
        &mut content,
        &serde_json::json!({
            "role":"user",
            "content": format!(
                "Review this frozen Blue oracle draft against the shared contracts and its cited bounded research evidence. Draft: {}. Research evidence: {}. This dogfood stage expects final JSON rather than a production submission tool. {}",
                serde_json::to_string(&draft)?,
                serde_json::to_string(&research_context)?,
                red_dogfood_contract()
            )
        }),
    )?;
    let red_projection = prepare_oracle_role_prompt(
        &plan,
        OracleRolePromptInput {
            role: OracleAgentRole::Red,
            append_only_context: Vec::new(),
            diagnostic_context: Vec::new(),
            current_request: red_request,
            policy: policy_document,
        },
    )?;
    let red_prompt = materialize_oracle_prompt(&content, &red_projection)?;
    let red_spec = red_prompt.native_spec(
        OracleAgentRole::Red,
        ModelName::new(model.wire_model().as_str())?,
        ModelOutputTokenLimit::new(live.red.output_tokens_per_turn)?,
    )?;
    let red_initial = codec.prepare_initial(&red_spec, red_prompt.user_text())?;
    let red_decision = red_projection.turn_input_decision(selection(&model)?);
    let red_turn = JsonTurnRuntime {
        events: &mut events,
        content: &mut content,
        transport: &mut transport,
        codec,
        spec: &red_spec,
        decision: &red_decision,
    }
    .run(
        red_initial,
        live.workflow.red_submission_repairs,
        "Red review",
        red_dogfood_contract(),
        RedDogfoodReviewV1::validate,
    )?;
    let mut red_usage = red_turn.usage;
    let mut red_repairs = red_turn.repairs;
    let mut red_continuation = red_turn.continuation;
    let mut review = red_turn.value;
    let review_descriptor =
        content.put::<RedDogfoodReviewArtifact>(&mut Cursor::new(cairn_codec::to_vec(&review)?))?;
    let mut review_ids = vec![review_descriptor.content_id];
    let mut adversarial_rounds = 0_u32;
    let mut stability_rechecks = 0_u32;
    let (debate_converged, debate_terminal_reason) = loop {
        match review.verdict {
            RedReviewVerdict::Revise => {
                let frozen_draft_id = draft_ids.last().map(ToString::to_string);
                tracing::warn!(
                    target: "cairn.oracle.debate",
                    event = "red_blockers_reported",
                    sample = sample.name,
                    frozen_draft_id,
                    blocker_count = review.blocking_findings.len(),
                    completed_revision_rounds = adversarial_rounds,
                    "Red requested a Blue revision"
                );
                if adversarial_rounds == live.workflow.adversarial_rounds {
                    break (
                        false,
                        format!(
                            "Red still reported {} blocker(s) after the configured {} Blue revision round(s)",
                            review.blocking_findings.len(),
                            live.workflow.adversarial_rounds
                        ),
                    );
                }
                let prior_draft_id = *draft_ids.last().ok_or("missing Blue draft identity")?;
                let revision_request = format!(
                    "Trusted Red review rejected frozen Blue draft {prior_draft_id}. Review: {}. Submit a changed complete replacement that addresses every blocking finding. Preserve valid content, state what changed in rationale/unverified fields, and follow this contract: {}",
                    serde_json::to_string(&review)?,
                    blue_dogfood_contract()
                );
                let next_blue = codec.append_user_text(&blue_continuation, &revision_request)?;
                let next_blue_native = codec.prepare_continuation(&spec, &next_blue)?;
                let blue_revision = JsonTurnRuntime {
                    events: &mut events,
                    content: &mut content,
                    transport: &mut transport,
                    codec,
                    spec: &spec,
                    decision: &second_decision,
                }
                .run(
                    next_blue_native,
                    live.workflow.blue_submission_repairs,
                    "Blue revision",
                    blue_dogfood_contract(),
                    |candidate: &BlueDogfoodDraftV1| {
                        candidate.validate()?;
                        let bytes = cairn_codec::to_vec(candidate)
                            .map_err(|error| format!("cannot encode candidate revision: {error}"))?;
                        let candidate_id = ContentId::<BlueDogfoodDraftArtifact>::derive(&bytes)
                            .map_err(|error| {
                                format!("cannot derive candidate revision identity: {error}")
                            })?;
                        if candidate_id == prior_draft_id {
                            return Err(format!(
                                "revision is byte-identical to rejected draft {prior_draft_id}; at least one blocker-relevant field must change"
                            ));
                        }
                        Ok(())
                    },
                )?;
                blue_usage.extend(blue_revision.usage);
                blue_repairs = blue_repairs.saturating_add(blue_revision.repairs);
                blue_continuation = blue_revision.continuation;
                draft = blue_revision.value;
                let descriptor = content.put::<BlueDogfoodDraftArtifact>(&mut Cursor::new(
                    cairn_codec::to_vec(&draft)?,
                ))?;
                draft_ids.push(descriptor.content_id);
                adversarial_rounds = adversarial_rounds.saturating_add(1);
                stability_rechecks = 0;
                tracing::info!(
                    target: "cairn.oracle.debate",
                    event = "blue_revision_accepted",
                    sample = sample.name,
                    prior_draft_id = %prior_draft_id,
                    revised_draft_id = %descriptor.content_id,
                    adversarial_round = adversarial_rounds,
                    "changed Blue revision accepted by dogfood validation"
                );

                let red_revision_request = format!(
                    "Blue submitted changed revision {} after your prior blockers. Re-evaluate the complete frozen revision, verify every prior blocker, and search for regressions. Draft: {}. Cited bounded research: {}. {}",
                    descriptor.content_id,
                    serde_json::to_string(&draft)?,
                    serde_json::to_string(&research_context)?,
                    red_dogfood_contract()
                );
                let next_red = codec.append_user_text(&red_continuation, &red_revision_request)?;
                let next_red_native = codec.prepare_continuation(&red_spec, &next_red)?;
                let red_revision = JsonTurnRuntime {
                    events: &mut events,
                    content: &mut content,
                    transport: &mut transport,
                    codec,
                    spec: &red_spec,
                    decision: &red_decision,
                }
                .run(
                    next_red_native,
                    live.workflow.red_submission_repairs,
                    "Red revision review",
                    red_dogfood_contract(),
                    RedDogfoodReviewV1::validate,
                )?;
                red_usage.extend(red_revision.usage);
                red_repairs = red_repairs.saturating_add(red_revision.repairs);
                red_continuation = red_revision.continuation;
                review = red_revision.value;
                let descriptor = content.put::<RedDogfoodReviewArtifact>(&mut Cursor::new(
                    cairn_codec::to_vec(&review)?,
                ))?;
                review_ids.push(descriptor.content_id);
            }
            RedReviewVerdict::Pass => {
                if stability_rechecks == live.workflow.stability_rechecks {
                    break (
                        true,
                        format!(
                            "Red reported no blockers and completed {stability_rechecks} configured stability recheck(s)"
                        ),
                    );
                }
                let focus = stability_focus(stability_rechecks);
                let frozen_draft_id = *draft_ids.last().ok_or("missing Blue draft identity")?;
                tracing::info!(
                    target: "cairn.oracle.debate",
                    event = "red_stability_recheck_started",
                    sample = sample.name,
                    frozen_draft_id = %frozen_draft_id,
                    recheck = stability_rechecks + 1,
                    configured_rechecks = live.workflow.stability_rechecks,
                    focus,
                    "Red stability recheck started"
                );
                let stability_request = format!(
                    "Perform stability recheck {} of {} over the same frozen Blue draft {}. Independently focus on {focus}. A prior pass is not authority: return revise if you find a concrete blocker, otherwise pass with only genuine advisories. Draft: {}. Cited bounded research: {}. {}",
                    stability_rechecks + 1,
                    live.workflow.stability_rechecks,
                    frozen_draft_id,
                    serde_json::to_string(&draft)?,
                    serde_json::to_string(&research_context)?,
                    red_dogfood_contract()
                );
                let next_red = codec.append_user_text(&red_continuation, &stability_request)?;
                let next_red_native = codec.prepare_continuation(&red_spec, &next_red)?;
                let red_recheck = JsonTurnRuntime {
                    events: &mut events,
                    content: &mut content,
                    transport: &mut transport,
                    codec,
                    spec: &red_spec,
                    decision: &red_decision,
                }
                .run(
                    next_red_native,
                    live.workflow.red_submission_repairs,
                    "Red stability review",
                    red_dogfood_contract(),
                    RedDogfoodReviewV1::validate,
                )?;
                red_usage.extend(red_recheck.usage);
                red_repairs = red_repairs.saturating_add(red_recheck.repairs);
                red_continuation = red_recheck.continuation;
                review = red_recheck.value;
                let descriptor = content.put::<RedDogfoodReviewArtifact>(&mut Cursor::new(
                    cairn_codec::to_vec(&review)?,
                ))?;
                review_ids.push(descriptor.content_id);
                stability_rechecks = stability_rechecks.saturating_add(1);
            }
        }
    };
    tracing::info!(
        target: "cairn.oracle.debate",
        event = "oracle_debate_completed",
        sample = sample.name,
        converged = debate_converged,
        adversarial_rounds,
        stability_rechecks,
        blue_submission_repairs = blue_repairs,
        red_submission_repairs = red_repairs,
        terminal_reason = %debate_terminal_reason,
        "oracle debate completed"
    );
    let ascend_c_test_blockers = [
        "Blue draft input is descriptive text, not a typed ABI-ordered input manifest",
        "the invocation names a framework call rather than an archived call-adapter contract",
        "expected dtype, shape, and values are not separate typed fields with materialized bytes",
        "no typed comparison artifact binds candidate output bytes to this draft",
    ];
    tracing::warn!(
        target: "cairn.oracle.dogfood",
        event = "oracle_downstream_not_ready",
        sample = sample.name,
        blocker_count = ascend_c_test_blockers.len(),
        "dogfood draft is not an executable Ascend C test artifact"
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model": model.wire_model().as_str(),
            "sample": sample.name,
            "blue_episode_id": plan.blue().episode_id(),
            "red_episode_is_distinct": plan.blue().episode_id() != plan.red().episode_id(),
            "research_request_id": search_request_id,
            "research_operation_result_id": result_id,
            "research_result_id": research_context.search_result(),
            "research_snippet_count": research_context.snippets().len(),
            "first_usage": first_usage,
            "blue_usage": blue_usage,
            "blue_submission_repairs": blue_repairs,
            "red_usage": red_usage,
            "red_submission_repairs": red_repairs,
            "blue_draft_ids": draft_ids,
            "blue_draft": draft,
            "red_review_ids": review_ids,
            "red_review": review,
            "adversarial_rounds": adversarial_rounds,
            "stability_rechecks": stability_rechecks,
            "debate_converged": debate_converged,
            "debate_terminal_reason": debate_terminal_reason,
            "ascend_c_test_readiness": {
                "ready": false,
                "blocking_contracts": ascend_c_test_blockers,
                "semantic_debate_is_not_admission": true
            },
            "restart_request_byte_identical": true,
            "research_tool_loop_completed": true,
            "upstream_license_queried": false,
            "upstream_bytes_promoted_to_corpus": false,
            "reasoning_content_printed": false
        }))?
    );
    Ok(())
}

fn blue_dogfood_contract() -> &'static str {
    "Return only one JSON object with exactly: schema_version=1; nonempty strings case_name, input, invocation, rationale, evidence_assessment; expectation as {kind:exact,output:string}, {kind:property,predicate:string}, or {kind:reject,error_behavior:string}; comparison as {kind:exact}, {kind:numeric,absolute_tolerance:string,relative_tolerance:string,ulp_tolerance:integer,nan_equal:boolean}, {kind:property}, or {kind:not-applicable}; assumptions and unverified as string arrays. Exact expectations require exact/numeric comparison, property requires property, and reject requires not-applicable. No markdown."
}

fn red_dogfood_contract() -> &'static str {
    "Return only one JSON object with exactly: schema_version=1; verdict pass or revise; strengths as a string array; blocking_findings and advisories as arrays of {kind:false-accept|false-reject|underspecified,detail:nonempty string}; recommended_revision as a nonempty string. Pass is valid exactly when blocking_findings is empty; otherwise revise. Never use placeholder findings. No markdown and no tool call."
}

fn stability_focus(index: u32) -> &'static str {
    match index % 3 {
        0 => {
            "false accepts, vacuity, missing companion controls, and concrete wrong implementations that may pass"
        }
        1 => {
            "false rejects, comparator overconstraint, legal implementation diversity, and intentionally unknown behavior"
        }
        _ => {
            "evidence relevance, unsupported assumptions, shape/axis arithmetic, dtype/layout semantics, and target leakage"
        }
    }
}

struct ValidatedJsonTurn<T> {
    value: T,
    continuation: NativeContinuation,
    usage: Vec<serde_json::Value>,
    repairs: u32,
}

struct JsonTurnRuntime<'a, P> {
    events: &'a mut SqliteEventStore,
    content: &'a mut SqliteContentStore,
    transport: &'a mut P,
    codec: NativeProtocolCodec,
    spec: &'a NativeRequestSpec,
    decision: &'a TurnInputDecision,
}

impl<P: ModelTransport> JsonTurnRuntime<'_, P> {
    #[expect(
        clippy::too_many_lines,
        reason = "the correction path keeps dispatch, decode, tool settlement, exact diagnostics, and continuation repair together"
    )]
    fn run<T>(
        &mut self,
        mut native: PreparedNativeRequest,
        max_repairs: u32,
        role: &str,
        contract: &str,
        validate: impl Fn(&T) -> Result<(), String>,
    ) -> Result<ValidatedJsonTurn<T>, Box<dyn std::error::Error>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut usage = Vec::new();
        for repair in 0..=max_repairs {
            let attempt = ModelAttemptId::new();
            tracing::info!(
                target: "cairn.oracle.submission",
                event = "structured_submission_attempt_started",
                role,
                model_attempt_id = %attempt,
                attempt = repair + 1,
                maximum_attempts = max_repairs + 1,
                "structured model submission attempt started"
            );
            let received = dispatch(
                self.events,
                self.content,
                self.transport,
                &stream(attempt)?,
                attempt,
                self.decision,
                &native,
            )?;
            usage.push(serde_json::to_value(received.usage())?);
            let decoded = self.codec.decode_recovered_received(
                self.events,
                self.content,
                received,
                &CommandId::new(),
                now()?,
            )?;
            let tool_names = decoded
                .semantic()
                .proposals()
                .iter()
                .map(|proposal| proposal.tool().as_str())
                .collect::<Vec<_>>();
            let mut semantic_bytes = Vec::new();
            self.content
                .write_to(&decoded.semantic().turn_id(), &mut semantic_bytes)?;
            let semantic: SemanticModelTurn = cairn_codec::from_slice(&semantic_bytes)?;
            let text = semantic
                .items
                .into_iter()
                .filter_map(|item| match item {
                    SemanticOutputItem::Text { text } => Some(text),
                    SemanticOutputItem::ToolCall { .. } => None,
                })
                .collect::<String>();
            let diagnostic = if !tool_names.is_empty() {
                format!(
                    "unexpected tool call(s) in this final-JSON stage: {}; no tool call was accepted",
                    tool_names.join(", ")
                )
            } else if text.trim().is_empty() {
                "the response contained no final semantic text; reasoning without a final submission is not accepted"
                    .to_owned()
            } else {
                match decode_json_object::<T>(&text) {
                    Ok(value) => match validate(&value) {
                        Ok(()) => {
                            tracing::info!(
                                target: "cairn.oracle.submission",
                                event = "structured_submission_accepted",
                                role,
                                model_attempt_id = %attempt,
                                repairs = repair,
                                "structured model submission accepted"
                            );
                            return Ok(ValidatedJsonTurn {
                                value,
                                continuation: decoded.continuation().clone(),
                                usage,
                                repairs: repair,
                            });
                        }
                        Err(error) => format!("typed V1 validation failed: {error}"),
                    },
                    Err(error) => format!(
                        "JSON decoding failed after {} semantic characters: {error}",
                        text.len()
                    ),
                }
            };
            tracing::warn!(
                target: "cairn.oracle.submission",
                event = "structured_submission_rejected",
                role,
                model_attempt_id = %attempt,
                attempt = repair + 1,
                maximum_attempts = max_repairs + 1,
                diagnostic = %diagnostic,
                "structured model submission rejected atomically"
            );
            if repair == max_repairs {
                return Err(format!(
                    "{role} exhausted {max_repairs} submission repair(s); last diagnostic: {diagnostic}"
                )
                .into());
            }
            let settled = if decoded.continuation().pending_call_ids().is_empty() {
                decoded.continuation().clone()
            } else {
                let results = decoded
                    .continuation()
                    .pending_call_ids()
                    .iter()
                    .cloned()
                    .map(|call_id| NativeToolResult {
                        call_id,
                        output: serde_json::json!({
                            "schema_version": 1,
                            "accepted": false,
                            "diagnostic": diagnostic
                        })
                        .to_string(),
                    })
                    .collect::<Vec<_>>();
                self.codec
                    .append_tool_results(decoded.continuation(), &results)?
            };
            let feedback = format!(
                "Trusted {role} submission validation rejected attempt {} of {}. Nothing from that submission was accepted. Diagnostic: {diagnostic}. Required contract: {contract} Correct every reported defect and return one complete replacement; do not repeat the rejected bytes or merely explain the error.",
                repair + 1,
                max_repairs + 1
            );
            let corrected = self.codec.append_user_text(&settled, &feedback)?;
            native = self.codec.prepare_continuation(self.spec, &corrected)?;
        }
        unreachable!("bounded repair loop always returns")
    }
}

fn decode_json_object<T>(text: &str) -> Result<T, Box<dyn std::error::Error>>
where
    T: serde::de::DeserializeOwned,
{
    Ok(serde_json::from_str(text.trim())?)
}

fn dogfood_sample(name: &str) -> Result<DogfoodSample, Box<dyn std::error::Error>> {
    let sample = match name {
        "sum-empty-axis" => DogfoodSample {
            name: "sum-empty-axis",
            operator: "sum",
            query: "reduction sum empty float32",
            caller: serde_json::json!({
                "kind":"caller-contract", "operator":"sum", "dtype":"f32",
                "shape":[2,0,3], "axis":1, "keepdim":false,
                "required":"each output cell reduces an empty axis and must use the additive identity",
                "unknowns":["target-device rounding"]
            }),
            source: serde_json::json!({
                "kind":"source-snapshot", "entry":"reduce_sum",
                "empty-input-behavior":"unspecified", "accumulation-order":"unspecified"
            }),
            task: "pin the output shape and value semantics for reduction over the zero-length axis; avoid a vacuous zero-element output",
        },
        "max-empty-axis" => DogfoodSample {
            name: "max-empty-axis",
            operator: "max",
            query: "test_empty_tensor_empty_slice",
            caller: serde_json::json!({
                "kind":"caller-contract", "operator":"max", "dtype":"f32",
                "shape":[2,0,3], "axis":1, "keepdim":false,
                "required":"no caller-supplied identity value exists",
                "unknowns":["error class and message text"]
            }),
            source: serde_json::json!({
                "kind":"source-snapshot", "entry":"reduce_max",
                "empty-input-behavior":"unspecified"
            }),
            task: "decide whether the empty-axis call returns or rejects, while avoiding brittle dependence on exact error prose",
        },
        "sum-noncontiguous" => DogfoodSample {
            name: "sum-noncontiguous",
            operator: "sum",
            query: "sum noncontiguous transpose test",
            caller: serde_json::json!({
                "kind":"caller-contract", "operator":"sum", "dtype":"f32",
                "logical-input":[[0,3],[1,4],[2,5]], "shape":[3,2],
                "strides-in-elements":[1,3], "axis":1, "keepdim":false,
                "required":"logical values, not contiguous reinterpretation, determine the result"
            }),
            source: serde_json::json!({
                "kind":"source-snapshot", "entry":"reduce_sum",
                "stride-support":"claimed", "layout-normalization":"unknown"
            }),
            task: "pin a layout-sensitive exact result that distinguishes correct strided indexing from contiguous reinterpretation",
        },
        "sum-nan" => DogfoodSample {
            name: "sum-nan",
            operator: "sum",
            query: "sum nan propagation test",
            caller: serde_json::json!({
                "kind":"caller-contract", "operator":"sum", "dtype":"f32",
                "shape":[4], "values":["1.0","NaN","-2.0","3.0"], "axis":0,
                "required":"ordinary sum is not a NaN-ignoring reduction",
                "unknowns":["NaN payload", "NaN sign"]
            }),
            source: serde_json::json!({
                "kind":"source-snapshot", "entry":"reduce_sum",
                "nan-policy":"unspecified", "accumulation-order":"unspecified"
            }),
            task: "pin NaN propagation without overconstraining payload bits or sign",
        },
        "matmul-zero-k" => DogfoodSample {
            name: "matmul-zero-k",
            operator: "matmul",
            query: "matmul zero size dimension test",
            caller: serde_json::json!({
                "kind":"caller-contract", "operator":"matmul", "dtype":"f32",
                "lhs-shape":[2,0], "rhs-shape":[0,3],
                "required":"the zero-length inner product uses the additive identity",
                "unknowns":["kernel dispatch for zero work"]
            }),
            source: serde_json::json!({
                "kind":"source-snapshot", "entry":"matmul",
                "zero-k-fast-path":"unknown"
            }),
            task: "pin output shape and every output value for a zero-K matrix product",
        },
        _ => {
            return Err(format!(
                "unknown sample {name}; expected sum-empty-axis, max-empty-axis, sum-noncontiguous, sum-nan, or matmul-zero-k"
            )
            .into());
        }
    };
    Ok(sample)
}

fn selection(
    model: &cairn_agent::ResolvedRuntimeModel,
) -> Result<ModelSelection, Box<dyn std::error::Error>> {
    Ok(ModelSelection {
        provider: ProviderName::new(model.provider().as_str())?,
        model: ModelName::new(model.wire_model().as_str())?,
        deployment: DeploymentName::new(model.deployment().as_str())?,
        adapter_version: AdapterVersion::new("native-protocol-v1")?,
    })
}

fn decision_after_research(
    content: &mut SqliteContentStore,
    model: &cairn_agent::ResolvedRuntimeModel,
    result: ContentId<OperationResult>,
    policy: ContentId<PolicyDocument>,
    tool_catalog: ContentId<ToolCatalog>,
) -> Result<TurnInputDecision, Box<dyn std::error::Error>> {
    Ok(TurnInputDecision {
        selection: selection(model)?,
        instructions: Vec::new(),
        tool_catalog,
        history: vec![put_json::<HistoryItem>(
            content,
            &serde_json::json!({"native_continuation":"recorded Blue research result"}),
        )?],
        context: Vec::new(),
        pending_results: vec![result],
        policy,
    })
}

fn dispatch<T: ModelTransport>(
    events: &mut SqliteEventStore,
    content: &mut SqliteContentStore,
    transport: &mut T,
    stream: &StreamId,
    attempt_id: ModelAttemptId,
    decision: &TurnInputDecision,
    native: &cairn_agent::PreparedNativeRequest,
) -> Result<ReceivedModelResponse, Box<dyn std::error::Error>> {
    let prepared = prepare_native_dispatch_request(content, decision, native)?;
    let authority = authorize_model_request(
        events,
        stream,
        ExpectedRevision::NoStream,
        &CommandId::new(),
        attempt_id,
        now()?,
        prepared,
    )?;
    let started = begin_model_dispatch(events, authority, &CommandId::new(), now()?)?;
    match execute_model_dispatch(
        events,
        content,
        transport,
        started,
        &CommandId::new(),
        now()?,
    )? {
        DispatchCompletion::Response(received) => Ok(received),
        DispatchCompletion::NotSent { diagnostic }
        | DispatchCompletion::Rejected { diagnostic }
        | DispatchCompletion::Ambiguous { diagnostic } => Err(diagnostic.into()),
    }
}

fn put_json<T: ContentType>(
    content: &mut SqliteContentStore,
    value: &serde_json::Value,
) -> Result<ContentId<T>, Box<dyn std::error::Error>> {
    let bytes = cairn_codec::to_vec(value)?;
    Ok(content.put::<T>(&mut Cursor::new(bytes))?.content_id)
}

fn stream(attempt_id: ModelAttemptId) -> Result<StreamId, Box<dyn std::error::Error>> {
    Ok(StreamId {
        kind: AggregateKind::new("oracle-blue-live-dogfood")?,
        id: AggregateId::new(attempt_id.to_string())?,
    })
}

fn now() -> Result<ObservedAtUnixMillis, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(ObservedAtUnixMillis::new(i64::try_from(millis)?))
}

#[cfg(test)]
mod tests {
    use cairn_agent::{
        ModelProtocolConfig, ModelTransportResponse, ResponsesReasoningReplay,
        ScriptedModelTransport, TransportError,
    };

    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RepairFixtureV1 {
        schema_version: u16,
        value: String,
    }

    #[test]
    fn rejected_json_receives_exact_feedback_and_repairs_in_the_same_continuation() {
        let directory = tempfile::tempdir().expect("tempdir");
        let mut content = SqliteContentStore::open(
            directory.path().join("content.db"),
            directory.path().join("cas"),
        )
        .expect("content");
        let mut events = SqliteEventStore::in_memory().expect("events");
        let instruction = put_json::<cairn_agent::InstructionBlock>(
            &mut content,
            &serde_json::json!({"text":"return the requested JSON"}),
        )
        .expect("instruction");
        let catalog = put_json::<ToolCatalog>(&mut content, &serde_json::json!({"tools":[]}))
            .expect("catalog");
        let history = put_json::<HistoryItem>(
            &mut content,
            &serde_json::json!({"role":"user","content":"submit"}),
        )
        .expect("history");
        let context =
            put_json::<ContextBlock>(&mut content, &serde_json::json!({"kind":"fixture"}))
                .expect("context");
        let policy =
            put_json::<PolicyDocument>(&mut content, &serde_json::json!({"network":"none"}))
                .expect("policy");
        let decision = TurnInputDecision {
            selection: ModelSelection {
                provider: ProviderName::new("recorded").expect("provider"),
                model: ModelName::new("recorded").expect("model"),
                deployment: DeploymentName::new("recorded").expect("deployment"),
                adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
            },
            instructions: vec![instruction],
            tool_catalog: catalog,
            history: vec![history],
            context: vec![context],
            pending_results: Vec::new(),
            policy,
        };
        let codec = NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
            store: false,
            reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
        })
        .expect("codec");
        let spec = NativeRequestSpec {
            wire_model: ModelName::new("recorded").expect("model"),
            instructions: "stable".to_owned(),
            tools: Vec::new(),
            max_output_tokens: ModelOutputTokenLimit::new(1_024).expect("limit"),
        };
        let initial = codec
            .prepare_initial(&spec, "submit fixture")
            .expect("initial");
        let mut calls = 0_u32;
        let mut transport = ScriptedModelTransport::new(
            |request: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
                calls = calls.saturating_add(1);
                let text = if calls == 1 {
                    "not JSON".to_owned()
                } else {
                    let request_text = String::from_utf8_lossy(request.request_bytes());
                    assert!(request_text.contains("JSON decoding failed"));
                    serde_json::json!({"schema_version":1,"value":"fixed"}).to_string()
                };
                Ok(ModelTransportResponse::without_usage(
                    serde_json::to_vec(&serde_json::json!({
                        "output":[{
                            "type":"message",
                            "id":format!("msg-{calls}"),
                            "phase":"final_answer",
                            "role":"assistant",
                            "status":"completed",
                            "content":[{"type":"output_text","text":text}]
                        }]
                    }))
                    .expect("response"),
                ))
            },
        );
        let repaired = JsonTurnRuntime {
            events: &mut events,
            content: &mut content,
            transport: &mut transport,
            codec,
            spec: &spec,
            decision: &decision,
        }
        .run(
            initial,
            1,
            "Blue fixture",
            "schema_version=1 and value=fixed",
            |value: &RepairFixtureV1| {
                if value.schema_version == 1 && value.value == "fixed" {
                    Ok(())
                } else {
                    Err("fixture fields are invalid".to_owned())
                }
            },
        )
        .expect("repair");
        assert_eq!(repaired.repairs, 1);
        assert_eq!(repaired.value.value, "fixed");
        assert_eq!(repaired.usage.len(), 2);
        assert!(repaired.continuation.pending_call_ids().is_empty());
        assert_eq!(calls, 2);
    }

    #[test]
    fn validators_return_actionable_field_and_cross_field_diagnostics() {
        assert!(
            decode_json_object::<RepairFixtureV1>(
                "Here is the answer: {\"schema_version\":1,\"value\":\"fixed\"}"
            )
            .is_err(),
            "surrounding prose must not be silently stripped from a structured submission"
        );
        let draft = BlueDogfoodDraftV1 {
            schema_version: 1,
            case_name: "case".to_owned(),
            input: "input".to_owned(),
            invocation: "invoke".to_owned(),
            expectation: DraftExpectationV1::Reject {
                error_behavior: "reject".to_owned(),
            },
            comparison: DraftComparisonV1::Exact,
            rationale: "rationale".to_owned(),
            evidence_assessment: "evidence".to_owned(),
            assumptions: Vec::new(),
            unverified: Vec::new(),
        };
        assert!(
            draft
                .validate()
                .expect_err("mismatch")
                .contains("expectation/comparison mismatch")
        );
        let review = RedDogfoodReviewV1 {
            schema_version: 1,
            verdict: RedReviewVerdict::Pass,
            strengths: Vec::new(),
            blocking_findings: vec![RedReviewFindingV1 {
                kind: RedReviewFindingKind::FalseAccept,
                detail: "wrong implementation passes".to_owned(),
            }],
            advisories: Vec::new(),
            recommended_revision: "fix it".to_owned(),
        };
        assert!(
            review
                .validate()
                .expect_err("mismatch")
                .contains("pass requires zero blockers")
        );
    }
}
