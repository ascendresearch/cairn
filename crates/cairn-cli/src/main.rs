use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
    str::FromStr,
    time::Duration,
};

use cairn_admission::{UserIntentAuthorityScopeV1, UserProvidedIntentClaimV1};
use cairn_migration::{
    IntentRecoveryRequestV1, SirCallerClaimId, SirHypothesisId, SirTaskArtifactPath,
    UserIntentDecisionRequestArtifact,
};
use cairn_protocol::{CommandId, ContentId, EventSequence, TaskId};
use cairn_sdk::{
    CairnClient, CairnClientConfigV1, CairnRequestV1, CairnResponseV1, TaskPhaseV1, TaskSourceV1,
    TaskSubmissionV1, UnixCairnClient,
};
use serde::{Serialize, de::DeserializeOwned};

const USAGE: &str = "usage: cairn-cli --config CLIENT.json task <submit SOURCE_ROOT RECOVERY.json|list|status TASK_ID|watch TASK_ID|cancel TASK_ID|intent-review TASK_ID|intent-select TASK_ID REQUEST_ID HYPOTHESIS_ID CLAIM_ID...|intent-keep-unknown TASK_ID REQUEST_ID CLAIM_ID...|intent-provide TASK_ID REQUEST_ID CLAIM.json CLAIM_ID...|intent-reconcile TASK_ID>";

#[tokio::main]
async fn main() -> ExitCode {
    match run(env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the reference CLI keeps its small command grammar in one auditable dispatch table"
)]
async fn run(arguments: Vec<String>) -> Result<(), String> {
    let [config_flag, config_path, group, command, rest @ ..] = arguments.as_slice() else {
        return Err(USAGE.into());
    };
    if config_flag != "--config" || group != "task" {
        return Err(USAGE.into());
    }
    let config: CairnClientConfigV1 = read_json(Path::new(config_path))?;
    let client = UnixCairnClient::new(config).map_err(display)?;
    match (command.as_str(), rest) {
        ("submit", [source_root, recovery]) => {
            let sources = collect_sources(Path::new(source_root))?;
            let recovery_request: IntentRecoveryRequestV1 = read_json(Path::new(recovery))?;
            let submission = TaskSubmissionV1::new(sources, recovery_request).map_err(display)?;
            exchange(
                &client,
                CairnRequestV1::SubmitTask {
                    command_id: CommandId::new(),
                    task_id: TaskId::new(),
                    submission,
                },
            )
            .await
        }
        ("list", []) => exchange(&client, CairnRequestV1::ListTasks).await,
        ("status", [task_id]) => {
            exchange(
                &client,
                CairnRequestV1::GetTask {
                    task_id: parse_task_id(task_id)?,
                },
            )
            .await
        }
        ("watch", [task_id]) => watch(&client, parse_task_id(task_id)?).await,
        ("cancel", [task_id]) => {
            exchange(
                &client,
                CairnRequestV1::CancelTask {
                    command_id: CommandId::new(),
                    task_id: parse_task_id(task_id)?,
                },
            )
            .await
        }
        ("intent-review", [task_id]) => {
            exchange(
                &client,
                CairnRequestV1::GetIntentReview {
                    task_id: parse_task_id(task_id)?,
                },
            )
            .await
        }
        ("intent-select", [task_id, request_id, hypothesis, claims @ ..]) if !claims.is_empty() => {
            exchange(
                &client,
                CairnRequestV1::SelectIntentHypothesis {
                    command_id: CommandId::new(),
                    task_id: parse_task_id(task_id)?,
                    request_id: ContentId::<UserIntentDecisionRequestArtifact>::from_str(
                        request_id,
                    )
                    .map_err(display)?,
                    hypothesis: SirHypothesisId::new(hypothesis.clone()).map_err(display)?,
                    authority_scope: parse_authority_scope(claims)?,
                },
            )
            .await
        }
        ("intent-keep-unknown", [task_id, request_id, claims @ ..]) if !claims.is_empty() => {
            exchange(
                &client,
                CairnRequestV1::KeepIntentUnknown {
                    command_id: CommandId::new(),
                    task_id: parse_task_id(task_id)?,
                    request_id: parse_intent_request_id(request_id)?,
                    authority_scope: parse_authority_scope(claims)?,
                },
            )
            .await
        }
        ("intent-provide", [task_id, request_id, claim, claims @ ..]) if !claims.is_empty() => {
            exchange(
                &client,
                CairnRequestV1::ProvideIntentClaim {
                    command_id: CommandId::new(),
                    task_id: parse_task_id(task_id)?,
                    request_id: parse_intent_request_id(request_id)?,
                    authority_scope: parse_authority_scope(claims)?,
                    claim: read_json::<UserProvidedIntentClaimV1>(Path::new(claim))?,
                },
            )
            .await
        }
        ("intent-reconcile", [task_id]) => {
            exchange(
                &client,
                CairnRequestV1::ReconcileIntentAdmission {
                    command_id: CommandId::new(),
                    task_id: parse_task_id(task_id)?,
                },
            )
            .await
        }
        _ => Err(USAGE.into()),
    }
}

async fn exchange(client: &UnixCairnClient, request: CairnRequestV1) -> Result<(), String> {
    let response = client.exchange(request).await.map_err(display)?;
    print_json(&response)
}

async fn watch(client: &UnixCairnClient, task_id: TaskId) -> Result<(), String> {
    let mut cursor: Option<EventSequence> = None;
    loop {
        let response = client
            .exchange(CairnRequestV1::GetTaskProgress {
                task_id,
                after_sequence: cursor,
            })
            .await
            .map_err(display)?;
        let CairnResponseV1::Progress { page } = response else {
            print_json(&response)?;
            return Err("server did not return a progress resource".into());
        };
        for item in &page.items {
            println!(
                "{}",
                String::from_utf8(cairn_codec::to_vec(item).map_err(display)?).map_err(display)?
            );
            cursor = Some(item.sequence);
        }
        if page.task.attention().is_some() || terminal_phase(page.task.phase()) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn terminal_phase(phase: TaskPhaseV1) -> bool {
    matches!(
        phase,
        TaskPhaseV1::OracleAccepted
            | TaskPhaseV1::OraclePartial
            | TaskPhaseV1::OracleRejected
            | TaskPhaseV1::Cancelled
            | TaskPhaseV1::Blocked
    )
}

fn collect_sources(root: &Path) -> Result<Vec<TaskSourceV1>, String> {
    let root = root.canonicalize().map_err(display)?;
    if !root.is_dir() {
        return Err("SOURCE_ROOT must be a directory".into());
    }
    let mut paths = Vec::new();
    visit(&root, &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(&root).map_err(display)?;
            let relative = relative
                .to_str()
                .ok_or_else(|| "task source path is not UTF-8".to_owned())?;
            let source_path =
                SirTaskArtifactPath::new(relative.replace(std::path::MAIN_SEPARATOR, "/"))
                    .map_err(display)?;
            let text = fs::read_to_string(path).map_err(display)?;
            TaskSourceV1::new(source_path, text).map_err(display)
        })
        .collect()
}

fn visit(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(display)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(display)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if entry.file_name() == ".git" {
            continue;
        }
        let file_type = entry.file_type().map_err(display)?;
        if file_type.is_symlink() {
            return Err(format!(
                "task bundle rejects symbolic link {}",
                entry.path().display()
            ));
        }
        if file_type.is_dir() {
            visit(&entry.path(), paths)?;
        } else if file_type.is_file() {
            paths.push(entry.path());
        } else {
            return Err(format!(
                "task bundle rejects non-regular file {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&fs::read(path).map_err(display)?).map_err(display)
}

fn parse_task_id(value: &str) -> Result<TaskId, String> {
    TaskId::from_str(value).map_err(display)
}

fn parse_intent_request_id(
    value: &str,
) -> Result<ContentId<UserIntentDecisionRequestArtifact>, String> {
    ContentId::from_str(value).map_err(display)
}

fn parse_authority_scope(claims: &[String]) -> Result<UserIntentAuthorityScopeV1, String> {
    let mut claims = claims
        .iter()
        .map(|claim| SirCallerClaimId::new(claim.clone()).map_err(display))
        .collect::<Result<Vec<_>, _>>()?;
    claims.sort();
    UserIntentAuthorityScopeV1::new(claims).map_err(display)
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    println!("{}", serde_json::to_string_pretty(value).map_err(display)?);
    Ok(())
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}
