//! Opt-in `DeepSeek` Responses conformance: live turn, durable restart, live continuation.

use std::{
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

use cairn_agent::{
    AdapterVersion, ContextBlock, DeploymentName, DispatchCompletion, HistoryItem,
    HttpModelTransport, InstructionBlock, ModelName, ModelOutputTokenLimit, ModelProtocolKind,
    ModelSelection, ModelTemplate, ModelTemplateRegistry, NativeProtocolCodec, NativeRequestSpec,
    OperationResult, PolicyDocument, ProviderName, ReceivedModelResponse, RuntimeModelCatalog,
    ToolCatalog, TurnInputDecision, authorize_model_request, begin_model_dispatch,
    execute_model_dispatch, prepare_native_dispatch_request,
};
use cairn_protocol::{
    AggregateId, AggregateKind, CommandId, ContentId, ContentType, ModelAttemptId,
    ObservedAtUnixMillis,
};
use cairn_record::{ContentStore, ExpectedRevision, StreamId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveConfig {
    schema_version: u16,
    max_output_tokens: u64,
    initial_user_text: String,
    followup_user_text: String,
}

#[allow(clippy::too_many_lines)] // Keep the end-to-end conformance narrative linear and auditable.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::current_dir()?;
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/live-conformance.example.json".to_owned());
    let live: LiveConfig = serde_json::from_slice(&std::fs::read(root.join(config_path))?)?;
    if live.schema_version != 1
        || live.initial_user_text.trim().is_empty()
        || live.followup_user_text.trim().is_empty()
    {
        return Err("invalid live conformance configuration".into());
    }

    let template: ModelTemplate = serde_json::from_slice(&std::fs::read(
        root.join("model-templates/deepseek/deepseek-v4-pro.json"),
    )?)?;
    let templates = ModelTemplateRegistry::from_templates([template])?;
    let catalog: RuntimeModelCatalog = serde_json::from_slice(&std::fs::read(
        root.join("config/runtime-models.example.json"),
    )?)?;
    let model = catalog.resolve(&templates, None)?;
    if model.protocol().kind() != ModelProtocolKind::OpenAiResponses {
        return Err("live conformance requires the Responses deployment".into());
    }
    let output_limit = ModelOutputTokenLimit::new(live.max_output_tokens)?;
    if output_limit > model.capabilities().max_output_tokens() {
        return Err("live max_output_tokens exceeds the model template".into());
    }
    let codec = NativeProtocolCodec::from_config(model.protocol())?;
    let request_spec = NativeRequestSpec {
        wire_model: model.wire_model().clone(),
        instructions: "This is a bounded Cairn transport conformance check.".to_owned(),
        tools: Vec::new(),
        max_output_tokens: output_limit,
    };
    let initial_native = codec.prepare_initial(&request_spec, &live.initial_user_text)?;
    let directory = tempfile::tempdir()?;
    let content_database = directory.path().join("content.db");
    let event_database = directory.path().join("events.db");
    let cas = directory.path().join("cas");
    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    let mut transport = HttpModelTransport::new(&model, &root)?;

    let first_attempt = ModelAttemptId::new();
    let first_stream = stream(first_attempt)?;
    let first_decision = decision(
        &mut content,
        &model,
        &serde_json::json!({"role":"user","content":live.initial_user_text}),
    )?;
    let first_received = dispatch(
        &mut events,
        &mut content,
        &mut transport,
        &first_stream,
        first_attempt,
        &first_decision,
        &initial_native,
        now()?,
    )?;
    let first_response_id = first_received.response_id();
    let first_usage = first_received.usage();
    let recorded = codec.record_received(
        &mut events,
        &mut content,
        &initial_native,
        first_received,
        &CommandId::new(),
        now()?,
    )?;
    let continuation_id = recorded.continuation_id();
    let before_restart = codec.prepare_continuation(
        &request_spec,
        &codec.append_user_text(recorded.continuation(), &live.followup_user_text)?,
    )?;

    drop(recorded);
    drop(events);
    drop(content);

    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    let (recovered_id, recovered) = codec
        .recover_recorded(&events, &content, &first_stream, first_attempt)?
        .ok_or("native continuation event was not recovered")?;
    if recovered_id != continuation_id {
        return Err("recovered continuation identity changed".into());
    }
    let recovered = codec.append_user_text(&recovered, &live.followup_user_text)?;
    let after_restart = codec.prepare_continuation(&request_spec, &recovered)?;
    if before_restart.request_bytes() != after_restart.request_bytes() {
        return Err("restart changed the next provider request bytes".into());
    }

    let second_attempt = ModelAttemptId::new();
    let second_stream = stream(second_attempt)?;
    let second_decision = decision(
        &mut content,
        &model,
        &serde_json::json!({
            "native_continuation_id": recovered_id.to_wire(),
            "new_user_text": live.followup_user_text
        }),
    )?;
    let second_received = dispatch(
        &mut events,
        &mut content,
        &mut transport,
        &second_stream,
        second_attempt,
        &second_decision,
        &after_restart,
        now()?,
    )?;
    let second_response_id = second_received.response_id();
    let second_usage = second_received.usage();
    let _second_recorded = codec.record_received(
        &mut events,
        &mut content,
        &after_restart,
        second_received,
        &CommandId::new(),
        now()?,
    )?;

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "model": model.wire_model().as_str(),
            "protocol": "openai_responses",
            "first_attempt_id": first_attempt,
            "first_response_id": first_response_id,
            "first_usage": first_usage,
            "continuation_id": continuation_id,
            "restart_request_byte_identical": true,
            "second_attempt_id": second_attempt,
            "second_response_id": second_response_id,
            "second_usage": second_usage,
            "thinking_or_answer_content_printed": false
        }))?
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)] // Mirrors the explicit durable dispatch capability boundary.
fn dispatch(
    events: &mut SqliteEventStore,
    content: &mut SqliteContentStore,
    transport: &mut HttpModelTransport,
    stream: &StreamId,
    attempt_id: ModelAttemptId,
    decision: &TurnInputDecision,
    native: &cairn_agent::PreparedNativeRequest,
    observed_at: ObservedAtUnixMillis,
) -> Result<ReceivedModelResponse, Box<dyn std::error::Error>> {
    let prepared = prepare_native_dispatch_request(content, decision, native)?;
    let authority = authorize_model_request(
        events,
        stream,
        ExpectedRevision::NoStream,
        &CommandId::new(),
        attempt_id,
        observed_at,
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
        DispatchCompletion::NotSent => Err("live request was not sent".into()),
        DispatchCompletion::Rejected => Err("live request was rejected".into()),
        DispatchCompletion::Ambiguous => Err("live request outcome is ambiguous".into()),
    }
}

fn decision(
    content: &mut SqliteContentStore,
    model: &cairn_agent::ResolvedRuntimeModel,
    history: &serde_json::Value,
) -> Result<TurnInputDecision, Box<dyn std::error::Error>> {
    Ok(TurnInputDecision {
        selection: ModelSelection {
            provider: ProviderName::new(model.provider().as_str())?,
            model: ModelName::new(model.wire_model().as_str())?,
            deployment: DeploymentName::new(model.deployment().as_str())?,
            adapter_version: AdapterVersion::new("native-protocol-v1")?,
        },
        instructions: vec![put_json::<InstructionBlock>(
            content,
            &serde_json::json!({"text":"bounded live conformance"}),
        )?],
        tool_catalog: put_json::<ToolCatalog>(content, &serde_json::json!({"tools":[]}))?,
        history: vec![put_json::<HistoryItem>(content, history)?],
        context: Vec::<ContentId<ContextBlock>>::new(),
        pending_results: Vec::<ContentId<OperationResult>>::new(),
        policy: put_json::<PolicyDocument>(
            content,
            &serde_json::json!({"network":"configured_provider_only"}),
        )?,
    })
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
        kind: AggregateKind::new("model-live-conformance")?,
        id: AggregateId::new(attempt_id.to_string())?,
    })
}

fn now() -> Result<ObservedAtUnixMillis, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(ObservedAtUnixMillis::new(i64::try_from(millis)?))
}
