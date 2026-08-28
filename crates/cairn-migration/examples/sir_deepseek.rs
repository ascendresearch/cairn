//! Opt-in live `DeepSeek` SIR proposal run for one explicit user-owned task directory.

use std::{
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, DeploymentName, EpisodeBudget,
    EpisodeDeadlineUnixMillis, EpisodeProviderTokenLimit, EpisodeStepLimit,
    EpisodeToolOperationLimit, HttpModelTransport, ModelName, ModelOutputTokenLimit,
    ModelProtocolKind, ModelSelection, ModelTemplate, ModelTemplateRegistry, NativeProtocolCodec,
    ProviderName, ResolvedRuntimeModelArtifact, RuntimeModelAlias, RuntimeModelCatalog,
    recover_agent_episode,
};
use cairn_migration::{
    SirEpisodeRunInput, SirReadByteLimit, SirReadLineLimit, SirTaskByteLimit, SirTaskFileLimit,
    SirTaskLimits, SirTaskWorkspace, run_sir_episode,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveSirConfig {
    schema_version: u16,
    runtime_model: String,
    max_output_tokens: u64,
    max_steps: u32,
    max_tool_operations: u32,
    max_provider_tokens: u64,
    deadline_seconds: u64,
    max_task_files: u32,
    max_task_bytes: u64,
    max_read_lines: u32,
    max_read_bytes: u64,
    state_root: String,
}

struct ResolvedLiveSirConfig {
    runtime_model: RuntimeModelAlias,
    max_output_tokens: ModelOutputTokenLimit,
    budget: EpisodeBudget,
    task_limits: SirTaskLimits,
    state_root: PathBuf,
}

impl LiveSirConfig {
    fn resolve(self, root: &Path) -> Result<ResolvedLiveSirConfig, Box<dyn std::error::Error>> {
        if self.schema_version != 1 {
            return Err("SIR live configuration must use schema_version 1".into());
        }
        let relative_state_root = Path::new(&self.state_root);
        if !relative_state_root.starts_with(Path::new(".cairn/runs"))
            || relative_state_root
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err("state_root must be a relative path below .cairn/runs".into());
        }
        let deadline_delta = self
            .deadline_seconds
            .checked_mul(1_000)
            .ok_or("deadline_seconds overflowed milliseconds")?;
        let deadline = now_millis()?
            .checked_add(i64::try_from(deadline_delta)?)
            .ok_or("deadline timestamp overflowed")?;
        Ok(ResolvedLiveSirConfig {
            runtime_model: RuntimeModelAlias::new(self.runtime_model)?,
            max_output_tokens: ModelOutputTokenLimit::new(self.max_output_tokens)?,
            budget: EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(self.max_steps)?),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(
                    self.max_tool_operations,
                )),
                provider_token_limit: Some(EpisodeProviderTokenLimit::new(
                    self.max_provider_tokens,
                )?),
                deadline_unix_ms: Some(EpisodeDeadlineUnixMillis::new(deadline)),
                external_meter_limits: None,
            },
            task_limits: SirTaskLimits {
                max_files: SirTaskFileLimit::new(self.max_task_files)?,
                max_task_bytes: SirTaskByteLimit::new(self.max_task_bytes)?,
                max_read_lines: SirReadLineLimit::new(self.max_read_lines)?,
                max_read_bytes: SirReadByteLimit::new(self.max_read_bytes)?,
            },
            state_root: root.join(relative_state_root),
        })
    }
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cairn_observability::init("sir-deepseek-live")?;
    let root = std::env::current_dir()?;
    let mut args = std::env::args().skip(1);
    let task_root = args
        .next()
        .ok_or("usage: sir_deepseek <task-root> [config-path]")?;
    let config_path = args
        .next()
        .unwrap_or_else(|| "config/sir-deepseek.example.json".to_owned());
    if args.next().is_some() {
        return Err("usage: sir_deepseek <task-root> [config-path]".into());
    }
    let raw: LiveSirConfig = serde_json::from_slice(&fs::read(root.join(config_path))?)?;
    let live = raw.resolve(&root)?;

    let template: ModelTemplate = serde_json::from_slice(&fs::read(
        root.join("model-templates/deepseek/deepseek-v4-pro.json"),
    )?)?;
    let templates = ModelTemplateRegistry::from_templates([template])?;
    let catalog: RuntimeModelCatalog =
        serde_json::from_slice(&fs::read(root.join("config/runtime-models.example.json"))?)?;
    let model = catalog.resolve(&templates, Some(&live.runtime_model))?;
    if model.protocol().kind() != ModelProtocolKind::OpenAiResponses {
        return Err("SIR live run requires an OpenAI Responses deployment".into());
    }
    if live.max_output_tokens > model.capabilities().max_output_tokens() {
        return Err("SIR max_output_tokens exceeds the selected model capability".into());
    }

    let workspace = SirTaskWorkspace::load(&root.join(task_root), live.task_limits)?;
    let episode_id = EpisodeId::new();
    let state = live.state_root.join(episode_id.to_string());
    fs::create_dir_all(&state)?;
    let content_database = state.join("content.db");
    let event_database = state.join("events.db");
    let cas = state.join("cas");
    let mut content = SqliteContentStore::open(&content_database, &cas)?;
    let mut events = SqliteEventStore::open(&event_database)?;
    let model_configuration = archive_model_configuration(&mut content, &model)?;
    let codec = NativeProtocolCodec::from_config(model.protocol())?;
    let mut transport = HttpModelTransport::new(&model, &root)?;
    let outcome = run_sir_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec,
        workspace,
        SirEpisodeRunInput {
            task_id: TaskId::new(),
            episode_id,
            model_configuration,
            selection: ModelSelection {
                provider: ProviderName::new(model.provider().as_str())?,
                model: ModelName::new(model.wire_model().as_str())?,
                deployment: DeploymentName::new(model.deployment().as_str())?,
                adapter_version: AdapterVersion::new("native-protocol-v1")?,
            },
            budget: live.budget,
            max_output_tokens: live.max_output_tokens,
            task_limits: live.task_limits,
        },
    )?;

    drop(transport);
    drop(events);
    drop(content);

    let mut reopened_content = SqliteContentStore::open(&content_database, &cas)?;
    let reopened_events = SqliteEventStore::open(&event_database)?;
    let recovered = recover_agent_episode(
        &reopened_events,
        &mut reopened_content,
        &AgentEpisode::new(episode_id)?,
    )?;
    let AgentEpisodeState::Completed {
        reason,
        steps_started,
    } = recovered
    else {
        return Err("reopened SIR episode was not durably completed".into());
    };
    if reason != outcome.completion_reason() || steps_started != outcome.steps_started() {
        return Err("reopened SIR terminal projection changed".into());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "runtime_model": model.alias().as_str(),
            "wire_model": model.wire_model().as_str(),
            "deployment": model.deployment().as_str(),
            "episode_id": outcome.episode_id(),
            "task_bundle": outcome.task_bundle(),
            "proposal_id": outcome.proposal_id(),
            "completion_reason": outcome.completion_reason(),
            "steps_started": outcome.steps_started(),
            "state_directory": state,
            "terminal_restart_recovered": true,
            "provider_response_or_reasoning_printed": false
        }))?
    );
    Ok(())
}

fn archive_model_configuration(
    content: &mut SqliteContentStore,
    model: &cairn_agent::ResolvedRuntimeModel,
) -> Result<ContentId<ResolvedRuntimeModelArtifact>, Box<dyn std::error::Error>> {
    let expected = model.content_id()?;
    let archived = content
        .put::<ResolvedRuntimeModelArtifact>(&mut Cursor::new(model.canonical_bytes()?))?
        .content_id;
    if archived != expected {
        return Err("resolved runtime-model identity changed while archiving".into());
    }
    Ok(archived)
}

fn now_millis() -> Result<i64, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(i64::try_from(millis)?)
}
