//! Opt-in Blue Oracle dogfood: real model, recorded upstream research, durable restart.

use std::{
    io::Cursor,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use cairn_agent::{
    AdapterVersion, ContextBlock, DeploymentName, DispatchCompletion, EpisodeBudget,
    EpisodeProviderTokenLimit, EpisodeStepLimit, EpisodeToolOperationLimit, HistoryItem,
    HttpModelTransport, InstructionBlock, ModelName, ModelOutputTokenLimit, ModelProtocolKind,
    ModelSelection, ModelTemplate, ModelTemplateRegistry, NativeProtocolCodec, NativeToolResult,
    OperationResult, PolicyDocument, ProviderName, ReceivedModelResponse,
    ResolvedRuntimeModelArtifact, RuntimeModelCatalog, SemanticModelTurn, SemanticOutputItem,
    ToolCatalog, TurnInputDecision, authorize_model_request, authorize_tool_operation,
    begin_model_dispatch, begin_tool_operation, execute_model_dispatch, execute_tool_operation,
    prepare_native_dispatch_request, prepare_tool_operation,
};
use cairn_migration::{
    ExternalResearchPolicy, ExternalResearchProvider, ExternalResearchProviderError,
    ExternalTestCaseV1, ExternalTestResearchContextV1, ExternalTestSearchGateway,
    ExternalTestSearchRequestArtifact, ExternalTestSearchRequestV1, ExternalTestSearchResultV1,
    GitHubBlobIdentity, GitHubExternalResearchProvider, GitHubRepository, OracleAgentRole,
    OracleRoleEpisodeInput, OracleRolePromptInput, OracleSearchPlanInput, OracleSearchPlanV1,
    RecordedExternalResearchExchange, RecordedExternalResearchProvider, SearchResultLimit,
    SourcePath, archive_external_test_evidence, archive_oracle_role_tool_catalog,
    external_test_search_registration, materialize_oracle_prompt, prepare_oracle_role_episode,
    prepare_oracle_role_prompt,
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
    fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.recommended_revision.trim().is_empty()
            || self.strengths.len() > 16
            || self.strengths.iter().any(|item| item.trim().is_empty())
            || [&self.blocking_findings, &self.advisories]
                .iter()
                .any(|items| {
                    items.len() > 16 || items.iter().any(|finding| finding.detail.trim().is_empty())
                })
            || matches!(self.verdict, RedReviewVerdict::Pass) != self.blocking_findings.is_empty()
        {
            return Err("Red dogfood review violates the bounded V1 shape");
        }
        Ok(())
    }
}

impl BlueDogfoodDraftV1 {
    fn validate(&self) -> Result<(), &'static str> {
        let required = [
            self.case_name.as_str(),
            self.input.as_str(),
            self.invocation.as_str(),
            self.rationale.as_str(),
            self.evidence_assessment.as_str(),
        ];
        if self.schema_version != 1
            || required
                .iter()
                .any(|value| value.trim().is_empty() || value.chars().any(char::is_control))
            || self.assumptions.len() > 16
            || self.unverified.len() > 16
        {
            return Err("Blue dogfood draft violates the bounded V1 shape");
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
            return Err("Blue dogfood expectation and comparison are incoherent");
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
    research: ResearchConfig,
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
    let root = std::env::current_dir()?;
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/oracle-blue-dogfood.example.json".to_owned());
    let sample_name = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "sum-empty-axis".to_owned());
    let sample = dogfood_sample(&sample_name)?;
    let live: LiveConfig = serde_json::from_slice(&std::fs::read(root.join(config_path))?)?;
    if live.schema_version != 1 || live.research.repositories.is_empty() {
        return Err("unsupported live dogfood configuration".into());
    }
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

    let common = put_json::<InstructionBlock>(
        &mut content,
        &serde_json::json!({"text":"Treat upstream source only as research. Independently synthesize Cairn test semantics; never claim that origin grants truth."}),
    )?;
    let blue_instruction = put_json::<InstructionBlock>(
        &mut content,
        &serde_json::json!({"text":"You are Blue. First use the bounded research tool exactly once. After its result, independently author one oracle case. Return only a JSON object with exactly these fields: schema_version=1; case_name, input, invocation, rationale, evidence_assessment as nonempty strings; expectation as one tagged object {kind: exact, output: string} or {kind: property, predicate: string} or {kind: reject, error_behavior: string}; comparison as one tagged object {kind: exact}, {kind: numeric, absolute_tolerance: decimal string, relative_tolerance: decimal string, ulp_tolerance: integer, nan_equal: boolean}, {kind: property}, or {kind: not-applicable}; assumptions and unverified as string arrays. Exact expectations require exact or numeric comparison, properties require property comparison, and rejection requires not-applicable. Do not copy an upstream test and do not emit markdown."}),
    )?;
    let red_instruction = put_json::<InstructionBlock>(
        &mut content,
        &serde_json::json!({"text":"You are Red. You cannot access Blue private history. Attack only the frozen Blue draft and shared caller/source contracts. Return only a JSON object with exactly: schema_version=1; verdict as pass or revise; strengths as a string array; blocking_findings and advisories as arrays of objects with exactly kind (false-accept, false-reject, or underspecified) and nonempty detail; recommended_revision as a nonempty string. Put only defects that make this draft unsafe to admit in blocking_findings; put optional hardening or intentionally out-of-contract concerns in advisories. verdict must be pass exactly when blocking_findings is empty, otherwise revise. Never use placeholder findings such as 'none identified'. Do not emit markdown and do not call tools."}),
    )?;
    let caller = put_json::<ContextBlock>(&mut content, &sample.caller)?;
    let source = put_json::<ContextBlock>(&mut content, &sample.source)?;
    let repository_request = serde_json::to_string(&live.research.repositories)?;
    let request = put_json::<HistoryItem>(
        &mut content,
        &serde_json::json!({
            "role":"user",
            "content": format!(
                "For sample '{}', first call oracle_search_external_tests with schema_version 1, query '{}', repositories {repository_request}, and max_results {}. Then use the result only as research and independently produce this oracle draft: {}",
                sample.name,
                sample.query,
                live.research.max_results_per_search
                ,sample.task
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
    let second_attempt = ModelAttemptId::new();
    let second_stream = stream(second_attempt)?;
    let second_decision = decision_after_research(
        &mut content,
        &model,
        result_id,
        policy_document,
        plan.blue().tool_catalog(),
    )?;
    let second_received = dispatch(
        &mut events,
        &mut content,
        &mut transport,
        &second_stream,
        second_attempt,
        &second_decision,
        &after_restart,
    )?;
    let second_usage = second_received.usage();
    let second_decoded = codec.decode_recovered_received(
        &mut events,
        &mut content,
        second_received,
        &CommandId::new(),
        now()?,
    )?;
    if !second_decoded.semantic().proposals().is_empty() {
        return Err("Blue called another tool instead of completing the research synthesis".into());
    }
    let mut semantic_bytes = Vec::new();
    content.write_to(&second_decoded.semantic().turn_id(), &mut semantic_bytes)?;
    let semantic: SemanticModelTurn = cairn_codec::from_slice(&semantic_bytes)?;
    let answers = semantic
        .items
        .into_iter()
        .filter_map(|item| match item {
            SemanticOutputItem::Text { text } => Some(text),
            SemanticOutputItem::ToolCall { .. } => None,
        })
        .collect::<Vec<_>>();
    if answers.is_empty() {
        return Err("Blue returned no semantic draft body".into());
    }
    let draft_text = answers.join("");
    let draft: BlueDogfoodDraftV1 = decode_draft(&draft_text).map_err(|error| {
        format!(
            "Blue draft decode failed after {} semantic characters: {error}",
            draft_text.len()
        )
    })?;
    draft.validate()?;
    let draft_bytes = cairn_codec::to_vec(&draft)?;
    let draft_descriptor =
        content.put::<BlueDogfoodDraftArtifact>(&mut Cursor::new(draft_bytes))?;
    let red_request = put_json::<HistoryItem>(
        &mut content,
        &serde_json::json!({
            "role":"user",
            "content": format!(
                "Review this frozen Blue oracle draft against the shared contracts and its cited bounded research evidence. Draft: {}. Research evidence: {}",
                serde_json::to_string(&draft)?,
                serde_json::to_string(&research_context)?
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
    let red_attempt = ModelAttemptId::new();
    let red_received = dispatch(
        &mut events,
        &mut content,
        &mut transport,
        &stream(red_attempt)?,
        red_attempt,
        &red_projection.turn_input_decision(selection(&model)?),
        &red_initial,
    )?;
    let red_usage = red_received.usage();
    let red_decoded = codec.decode_recovered_received(
        &mut events,
        &mut content,
        red_received,
        &CommandId::new(),
        now()?,
    )?;
    if !red_decoded.semantic().proposals().is_empty() {
        return Err("Red called a tool instead of reviewing the frozen draft".into());
    }
    let mut red_semantic_bytes = Vec::new();
    content.write_to(&red_decoded.semantic().turn_id(), &mut red_semantic_bytes)?;
    let red_semantic: SemanticModelTurn = cairn_codec::from_slice(&red_semantic_bytes)?;
    let red_text = red_semantic
        .items
        .into_iter()
        .filter_map(|item| match item {
            SemanticOutputItem::Text { text } => Some(text),
            SemanticOutputItem::ToolCall { .. } => None,
        })
        .collect::<String>();
    let review: RedDogfoodReviewV1 = decode_json_object(&red_text).map_err(|error| {
        format!(
            "Red review decode failed after {} semantic characters: {error}",
            red_text.len()
        )
    })?;
    review.validate()?;
    let review_descriptor =
        content.put::<RedDogfoodReviewArtifact>(&mut Cursor::new(cairn_codec::to_vec(&review)?))?;
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
            "second_usage": second_usage,
            "red_usage": red_usage,
            "blue_draft_id": draft_descriptor.content_id,
            "blue_draft": draft,
            "red_review_id": review_descriptor.content_id,
            "red_review": review,
            "restart_request_byte_identical": true,
            "research_tool_loop_completed": true,
            "upstream_license_queried": false,
            "upstream_bytes_promoted_to_corpus": false,
            "reasoning_content_printed": false
        }))?
    );
    Ok(())
}

fn decode_draft(text: &str) -> Result<BlueDogfoodDraftV1, Box<dyn std::error::Error>> {
    decode_json_object(text)
}

fn decode_json_object<T>(text: &str) -> Result<T, Box<dyn std::error::Error>>
where
    T: serde::de::DeserializeOwned,
{
    let trimmed = text.trim();
    let json = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        trimmed
    } else {
        let start = trimmed.find('{').ok_or("model output has no JSON object")?;
        let end = trimmed
            .rfind('}')
            .ok_or("model output has no JSON object")?;
        &trimmed[start..=end]
    };
    Ok(serde_json::from_str(json)?)
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

fn dispatch(
    events: &mut SqliteEventStore,
    content: &mut SqliteContentStore,
    transport: &mut HttpModelTransport,
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
