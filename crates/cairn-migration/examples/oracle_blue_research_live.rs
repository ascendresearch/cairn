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
    ResolvedRuntimeModelArtifact, RuntimeModelCatalog, ToolCatalog, TurnInputDecision,
    authorize_model_request, authorize_tool_operation, begin_model_dispatch, begin_tool_operation,
    execute_model_dispatch, execute_tool_operation, prepare_native_dispatch_request,
    prepare_tool_operation,
};
use cairn_migration::{
    ExternalResearchPolicy, ExternalResearchProvider, ExternalResearchProviderError,
    ExternalTestCaseV1, ExternalTestSearchGateway, ExternalTestSearchRequestArtifact,
    ExternalTestSearchRequestV1, ExternalTestSearchResultV1, GitHubBlobIdentity,
    GitHubExternalResearchProvider, GitHubRepository, OracleAgentRole, OracleRoleEpisodeInput,
    OracleRolePromptInput, OracleSearchPlanInput, OracleSearchPlanV1,
    RecordedExternalResearchExchange, RecordedExternalResearchProvider, SearchResultLimit,
    SourcePath, archive_oracle_role_tool_catalog, external_test_search_registration,
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
use serde::Deserialize;

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
        &serde_json::json!({"text":"You are Blue. First use the bounded research tool exactly once. After its result, explain one independently authored empty-reduction test idea without calling another tool."}),
    )?;
    let red_instruction = put_json::<InstructionBlock>(
        &mut content,
        &serde_json::json!({"text":"You are Red and cannot access Blue private history."}),
    )?;
    let caller = put_json::<ContextBlock>(
        &mut content,
        &serde_json::json!({"kind":"caller-contract","operator":"sum","dtype":"f32","unknowns":["target-device rounding"]}),
    )?;
    let source = put_json::<ContextBlock>(
        &mut content,
        &serde_json::json!({"kind":"source-snapshot","entry":"reduce_sum","empty-input-behavior":"unspecified"}),
    )?;
    let repository_request = serde_json::to_string(&live.research.repositories)?;
    let request = put_json::<HistoryItem>(
        &mut content,
        &serde_json::json!({
            "role":"user",
            "content": format!(
                "Call oracle_search_external_tests with schema_version 1, query 'reduction sum empty float32', repositories {repository_request}, and max_results {}. Then use the returned research only to independently formulate a Cairn test idea.",
                live.research.max_results_per_search
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
        task_inputs: ContentId::<OracleTaskInputArtifact>::derive(b"blue dogfood inputs")?,
        declared_domain: ContentId::<DeclaredDomainArtifact>::derive(b"blue dogfood domain")?,
        admission_policy: ContentId::<AdmissionPolicyArtifact>::derive(b"blue dogfood policy")?,
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
    let mut result_bytes = Vec::new();
    content.write_to(&result_id, &mut result_bytes)?;
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
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model": model.wire_model().as_str(),
            "blue_episode_id": plan.blue().episode_id(),
            "red_episode_is_distinct": plan.blue().episode_id() != plan.red().episode_id(),
            "research_request_id": search_request_id,
            "research_result_id": result_id,
            "first_usage": first_usage,
            "second_usage": second_usage,
            "restart_request_byte_identical": true,
            "research_tool_loop_completed": true,
            "upstream_license_queried": false,
            "upstream_bytes_promoted_to_corpus": false,
            "thinking_or_answer_content_printed": false
        }))?
    );
    Ok(())
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
