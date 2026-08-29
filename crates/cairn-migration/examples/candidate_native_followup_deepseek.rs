//! Opt-in live `DeepSeek` follow-up from one exact failed native Candidate build receipt.

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
    ProviderName, RuntimeModelAlias, RuntimeModelCatalog, recover_agent_episode,
};
use cairn_execution::{
    DockerImageId, ExecutionEvidenceArtifact, ExecutionReceipt, ExecutionReceiptArtifact,
    ExecutionStderrArtifact,
};
use cairn_migration::{
    CandidateBuildEnvironmentProfileV1, CandidateNativeFollowupEpisodeRunInput,
    CollectionCandidateRevisionArtifact, IntentRecoveryInputV1, SirReadByteLimit, SirReadLineLimit,
    SirResolvedRuntimeModelArtifact, SirTaskByteLimit, SirTaskFileLimit, SirTaskLimits,
    SirTaskWorkspace, prepare_candidate_native_build_diagnostic,
    prepare_candidate_native_revision_build_job, run_collection_candidate_native_followup_episode,
    validate_archived_collection_candidate_revision,
    validate_archived_collection_candidate_search_input,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveCandidateNativeFollowupConfig {
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

struct ResolvedLiveCandidateNativeFollowupConfig {
    runtime_model: RuntimeModelAlias,
    max_output_tokens: ModelOutputTokenLimit,
    budget: EpisodeBudget,
    task_limits: SirTaskLimits,
    state_root: PathBuf,
}

impl LiveCandidateNativeFollowupConfig {
    fn resolve(
        self,
        root: &Path,
    ) -> Result<ResolvedLiveCandidateNativeFollowupConfig, Box<dyn std::error::Error>> {
        if self.schema_version != 1 {
            return Err(
                "Candidate native follow-up configuration must use schema_version 1".into(),
            );
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
        Ok(ResolvedLiveCandidateNativeFollowupConfig {
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

#[derive(Deserialize)]
struct ControllerLocator {
    storage: ControllerStorageLocator,
}

#[derive(Deserialize)]
struct ControllerStorageLocator {
    content_database: PathBuf,
    content_directory: PathBuf,
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cairn_observability::init("candidate-native-followup-deepseek-live")?;
    let root = std::env::current_dir()?;
    let mut args = std::env::args().skip(1);
    let usage = "usage: candidate_native_followup_deepseek <task-root> <revision-state-dir> <previous-revision-id> <controller-config> <receipt-id> <sha256:image-id> [config-path]";
    let task_root = args.next().ok_or(usage)?;
    let revision_state = root.join(args.next().ok_or(usage)?);
    let previous_revision_id = args
        .next()
        .ok_or(usage)?
        .parse::<ContentId<CollectionCandidateRevisionArtifact>>()?;
    let controller_config_path = root.join(args.next().ok_or(usage)?);
    let receipt_id = args
        .next()
        .ok_or(usage)?
        .parse::<ContentId<ExecutionReceiptArtifact>>()?;
    let image = DockerImageId::new(args.next().ok_or(usage)?)?;
    let config_path = args
        .next()
        .unwrap_or_else(|| "config/candidate-native-followup-deepseek.example.json".to_owned());
    if args.next().is_some() {
        return Err(usage.into());
    }

    let raw: LiveCandidateNativeFollowupConfig =
        serde_json::from_slice(&fs::read(root.join(config_path))?)?;
    let live = raw.resolve(&root)?;
    let revision_content = SqliteContentStore::open(
        revision_state.join("content.db"),
        revision_state.join("cas"),
    )?;
    let previous_revision_bytes = read_content(&revision_content, &previous_revision_id)?;
    let previous_revision = validate_archived_collection_candidate_revision(
        &previous_revision_bytes,
        previous_revision_id,
    )?;
    let search_id = previous_revision.search_input();
    let search_bytes = read_content(&revision_content, &search_id)?;
    let search_input =
        validate_archived_collection_candidate_search_input(&search_bytes, search_id)?;
    let recovery_id = search_input.input().recovery_input();
    let recovery_bytes = read_content(&revision_content, &recovery_id)?;
    let recovery_input: IntentRecoveryInputV1 = cairn_codec::from_slice(&recovery_bytes)?;
    if cairn_codec::to_vec(&recovery_input)? != recovery_bytes
        || recovery_input.identity()? != recovery_id
    {
        return Err("recovery input is not canonical exact current-V1 material".into());
    }

    let (controller_database, controller_directory) = controller_storage(&controller_config_path)?;
    let controller_content = SqliteContentStore::open(controller_database, controller_directory)?;
    let receipt_bytes = read_content(&controller_content, &receipt_id)?;
    let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes)?;
    if cairn_codec::to_vec(&receipt)? != receipt_bytes {
        return Err("receipt is not canonical current-V1 material".into());
    }
    let stderr =
        read_content::<ExecutionStderrArtifact>(&controller_content, &receipt.stderr_id())?;
    let evidence =
        read_content::<ExecutionEvidenceArtifact>(&controller_content, &receipt.evidence_id())?;
    let build = prepare_candidate_native_revision_build_job(
        receipt.job_id(),
        &previous_revision_bytes,
        previous_revision_id,
        image,
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
    )?;
    let diagnostic = prepare_candidate_native_build_diagnostic(
        &build, receipt_id, &receipt, &stderr, &evidence,
    )?;

    let template: ModelTemplate = serde_json::from_slice(&fs::read(
        root.join("model-templates/deepseek/deepseek-v4-pro.json"),
    )?)?;
    let templates = ModelTemplateRegistry::from_templates([template])?;
    let catalog: RuntimeModelCatalog =
        serde_json::from_slice(&fs::read(root.join("config/runtime-models.example.json"))?)?;
    let model = catalog.resolve(&templates, Some(&live.runtime_model))?;
    if model.protocol().kind() != ModelProtocolKind::OpenAiResponses {
        return Err("Candidate native follow-up requires an OpenAI Responses deployment".into());
    }
    if live.max_output_tokens > model.capabilities().max_output_tokens() {
        return Err("Candidate native follow-up max_output_tokens exceeds model capability".into());
    }

    let workspace = SirTaskWorkspace::load(&root.join(task_root), live.task_limits)?;
    let episode_id = EpisodeId::new();
    if episode_id == previous_revision.episode_id() {
        return Err("native follow-up episode must be distinct from previous episode".into());
    }
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
    let outcome = run_collection_candidate_native_followup_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec,
        workspace,
        CandidateNativeFollowupEpisodeRunInput {
            search_input,
            recovery_input,
            previous_revision,
            previous_revision_id,
            build_diagnostic: diagnostic,
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
    let AgentEpisodeState::Completed {
        reason,
        steps_started,
    } = recover_agent_episode(
        &reopened_events,
        &mut reopened_content,
        &AgentEpisode::new(episode_id)?,
    )?
    else {
        return Err("reopened Candidate native follow-up was not durably completed".into());
    };
    if reason != outcome.completion_reason() || steps_started != outcome.steps_started() {
        return Err("reopened Candidate native follow-up terminal projection changed".into());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "runtime_model":model.alias().as_str(),
            "wire_model":model.wire_model().as_str(),
            "deployment":model.deployment().as_str(),
            "episode_id":outcome.episode_id(),
            "task_bundle":outcome.task_bundle(),
            "candidate_search_input":outcome.search_input(),
            "previous_revision":outcome.previous_revision_id(),
            "native_build_diagnostic":outcome.diagnostic_id(),
            "followup_revision_id":outcome.followup_id(),
            "followup_revision":outcome.followup(),
            "completion_reason":outcome.completion_reason(),
            "steps_started":outcome.steps_started(),
            "state_directory":state,
            "terminal_restart_recovered":true,
            "provider_raw_response_or_reasoning_printed":false,
            "build_or_execution_claimed":false
        }))?
    );
    Ok(())
}

fn controller_storage(
    config_path: &Path,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let locator: ControllerLocator = serde_json::from_slice(&fs::read(config_path)?)?;
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    let resolve = |path: PathBuf| {
        if path.is_relative() {
            base.join(path)
        } else {
            path
        }
    };
    Ok((
        resolve(locator.storage.content_database),
        resolve(locator.storage.content_directory),
    ))
}

fn read_content<T: ContentType>(
    content: &SqliteContentStore,
    id: &ContentId<T>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    content.write_to(id, &mut bytes)?;
    Ok(bytes)
}

fn archive_model_configuration(
    content: &mut SqliteContentStore,
    model: &cairn_agent::ResolvedRuntimeModel,
) -> Result<ContentId<SirResolvedRuntimeModelArtifact>, Box<dyn std::error::Error>> {
    let expected = model.content_id()?.to_wire();
    let archived = content
        .put::<SirResolvedRuntimeModelArtifact>(&mut Cursor::new(model.canonical_bytes()?))?
        .content_id;
    if archived.to_wire() != expected {
        return Err("resolved runtime-model identity changed while archiving".into());
    }
    Ok(archived)
}

fn now_millis() -> Result<i64, Box<dyn std::error::Error>> {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    Ok(i64::try_from(millis)?)
}
