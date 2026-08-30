use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use cairn_agent::{
    AdapterVersion, DeploymentName, EpisodeBudget, EpisodeStepLimit, EpisodeToolOperationLimit,
    MaterializedRequestArtifact, ModelName, ModelOutputTokenLimit, ModelSelection, ModelTemplate,
    ModelTemplateRegistry, ModelTransportResponse, ProviderName, RuntimeModelCatalog,
    ScriptedModelTransport, TransportError,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, IntentRecoveryRequestV1, ProposalHostBinaryIdentity,
    ProposalHostOutcomeV1, ProposalHostRequestV1, ProposalHostRoleRequestV1, ProposalHostRuntimeV1,
    ProposalHostTaskSnapshotV1, SirTaskLimits, SirTaskWorkspace, run_proposal_host_episode,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Serialize)]
struct RecordedExchangeWire {
    request_id: ContentId<MaterializedRequestArtifact>,
    response_bytes: Vec<u8>,
}

fn repository_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn host_binary_identity() -> ProposalHostBinaryIdentity {
    let bytes = fs::read(env!("CARGO_BIN_EXE_cairn-proposal-host")).expect("Host executable");
    ProposalHostBinaryIdentity::new(format!("sha256:{:x}", Sha256::digest(bytes)))
        .expect("Host binary identity")
}

fn prepare_host_operation(state_root: &Path, request: &ProposalHostRequestV1) {
    let state = state_root.join(request.runtime().episode_id().to_string());
    fs::create_dir_all(&state).expect("Host operation state");
    fs::write(
        state.join("invocation.v1.json"),
        cairn_codec::to_vec(request.runtime()).expect("runtime bytes"),
    )
    .expect("Host invocation marker");
}

fn resolved_model() -> cairn_agent::ResolvedRuntimeModel {
    let root = repository_root();
    let template: ModelTemplate = serde_json::from_slice(
        &fs::read(root.join("model-templates/deepseek/deepseek-v4-pro.json"))
            .expect("model template"),
    )
    .expect("template");
    let templates = ModelTemplateRegistry::from_templates([template]).expect("templates");
    let catalog: RuntimeModelCatalog = serde_json::from_slice(
        &fs::read(root.join("config/runtime-models.example.json")).expect("runtime catalog"),
    )
    .expect("catalog");
    catalog.resolve(&templates, None).expect("resolved model")
}

fn request_value() -> Value {
    json!({
        "schema_version":1,
        "caller":{
            "schema_version":1,
            "source_entry_point":"launch_transform",
            "arguments":[
                {"index":0,"name":"input","role":"input-buffer","data_type":"f32","shape":{"kind":"unknown-rank"},"valid_domain":"Readable binary32 elements."},
                {"index":1,"name":"output","role":"output-buffer","data_type":"f32","shape":{"kind":"unknown-rank"},"valid_domain":"Writable binary32 elements."},
                {"index":2,"name":"count","role":"scalar","data_type":"u32","shape":{"kind":"scalar"},"valid_domain":"Logical element count."}
            ],
            "error_behaviors":["Return the caller-visible launch status."],
            "claims":[{"id":"transform-elements","layer":"algorithm","statement":"Transform every logical input element.","references":[]}],
            "exclusions":[],
            "unknowns":[{"id":"launch-domain","kind":"shape-or-domain","question":"How does launch coverage relate to count?"}]
        },
        "target":{"soc":{"kind":"not-selected"},"toolchain":{"kind":"not-selected"},"environment":{"kind":"not-selected"}},
        "authorized_evidence":[],
        "prior_feedback":{"kind":"no-prior-feedback"}
    })
}

fn recovery_request() -> IntentRecoveryRequestV1 {
    cairn_codec::from_slice(&cairn_codec::to_vec(&request_value()).expect("request bytes"))
        .expect("request")
}

fn responses() -> Vec<Vec<u8>> {
    let read = serde_json::to_string(
        &json!({"schema_version":1,"path":"transform.cu","start_line":1,"line_count":10}),
    )
    .expect("read args");
    let submit = serde_json::to_string(&json!({
        "schema_version":1,
        "observed_facts":[
            {"id":"index-derived","statement":"The source derives an index from CUDA launch coordinates.","citations":[{"path":"transform.cu","start_line":2,"end_line":3}]},
            {"id":"output-write","statement":"The source copies the indexed input value to output.","citations":[{"path":"transform.cu","start_line":3,"end_line":3}]}
        ],
        "hypotheses":[
            {"id":"launched-indices","layer":"observable-contract","claim":"Only launched indices are required.","domain":"Indices covered by launch geometry.","supporting_evidence":[{"source":"observed-fact","observation":"index-derived"}],"counter_evidence":[{"source":"caller-claim","claim":"transform-elements"}]},
            {"id":"logical-count","layer":"algorithm","claim":"Every logical caller element is transformed.","domain":"Caller logical element domain.","supporting_evidence":[{"source":"caller-claim","claim":"transform-elements"},{"source":"observed-fact","observation":"output-write"}],"counter_evidence":[{"source":"observed-fact","observation":"index-derived"}]}
        ],
        "conflicts":[{"id":"coverage-conflict","statement":"Caller intent and visible launch-index behavior do not close coverage.","claims":[{"source":"hypothesis","hypothesis":"launched-indices"},{"source":"hypothesis","hypothesis":"logical-count"}],"evidence":[{"source":"caller-claim","claim":"transform-elements"},{"source":"observed-fact","observation":"index-derived"}]}],
        "unknowns":[{"id":"host-coverage","kind":"source-behavior","question":"What launch coverage does the host provide?","evidence":[{"source":"observed-fact","observation":"index-derived"}]}],
        "invariants":[{"id":"copy-value","statement":"Every admitted index copies its corresponding value.","evidence":[{"source":"observed-fact","observation":"output-write"}]}],
        "optimization_freedoms":[],
        "source_dispositions":[{"id":"launch-indexing","observation":"index-derived","disposition":"unknown-classification","rationale":"Host launch material is absent.","evidence":[{"source":"observed-fact","observation":"index-derived"}]}],
        "disambiguation_experiments":[{"id":"inspect-launch","targets":[{"kind":"conflict","conflict":"coverage-conflict"},{"kind":"unknown","unknown":"host-coverage"}],"plan":"Inspect the exact host launch.","predictions":["Count-bounded coverage supports logical-count.","Partial coverage supports launched-indices."]}]
    }))
    .expect("submit args");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"sir-read","name":"sir_read_task_artifact","arguments":read}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"sir-submit","name":"sir_submit_intent_hypotheses","arguments":submit}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"sir-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"submitted"}]}]})).expect("response"),
    ]
}

#[test]
#[allow(clippy::too_many_lines)]
fn child_process_consumes_canonical_recorded_sir_request_and_recovers_terminal() {
    let temporary = tempfile::tempdir().expect("temporary root");
    let task = temporary.path().join("task");
    fs::create_dir(&task).expect("task directory");
    fs::write(
        task.join("transform.cu"),
        "__global__ void transform(const float* input, float* output) {\n  int i = blockIdx.x * blockDim.x + threadIdx.x;\n  output[i] = input[i];\n}\n",
    )
    .expect("task source");
    let workspace = SirTaskWorkspace::load(&task, SirTaskLimits::default()).expect("workspace");
    let model = resolved_model();
    let episode_id = EpisodeId::new();
    let request = ProposalHostRequestV1::new(
        ProposalHostRuntimeV1::new(
            episode_id,
            host_binary_identity(),
            ContentId::<AgentResolvedRuntimeModelArtifact>::derive(
                &model.canonical_bytes().expect("model bytes"),
            )
            .expect("migration model marker"),
            ModelSelection {
                provider: ProviderName::new(model.provider().as_str()).expect("provider"),
                model: ModelName::new(model.wire_model().as_str()).expect("model"),
                deployment: DeploymentName::new(model.deployment().as_str()).expect("deployment"),
                adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
            },
            EpisodeBudget {
                step_limit: Some(EpisodeStepLimit::new(6).expect("steps")),
                tool_operation_limit: Some(EpisodeToolOperationLimit::new(12)),
                provider_token_limit: None,
                deadline_unix_ms: None,
                external_meter_limits: None,
            },
            ModelOutputTokenLimit::new(16_384).expect("output limit"),
            SirTaskLimits::default(),
        ),
        ProposalHostRoleRequestV1::Sir {
            task_id: TaskId::new(),
            recovery_request: recovery_request(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
        },
    )
    .expect("Host request");

    let capture = temporary.path().join("capture");
    fs::create_dir(&capture).expect("capture directory");
    let mut content =
        SqliteContentStore::open(capture.join("content.db"), capture.join("cas")).expect("content");
    let mut events = SqliteEventStore::open(capture.join("events.db")).expect("events");
    let scripted_responses = responses();
    let mut response_index = 0_usize;
    let mut exchanges = Vec::new();
    {
        let mut scripted = ScriptedModelTransport::new(
            |prepared: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
                let response = scripted_responses
                    .get(response_index)
                    .expect("scripted response")
                    .clone();
                response_index += 1;
                exchanges.push(RecordedExchangeWire {
                    request_id: prepared.request_id(),
                    response_bytes: response.clone(),
                });
                Ok(ModelTransportResponse::without_usage(response))
            },
        );
        run_proposal_host_episode(
            &mut events,
            &mut content,
            &mut scripted,
            cairn_agent::NativeProtocolCodec::from_config(model.protocol()).expect("codec"),
            request.clone(),
        )
        .expect("capture Host run");
    }

    let model_path = temporary.path().join("model.json");
    fs::write(&model_path, model.canonical_bytes().expect("model bytes")).expect("model file");
    let exchanges_path = temporary.path().join("exchanges.json");
    fs::write(
        &exchanges_path,
        cairn_codec::to_vec(&exchanges).expect("exchange bytes"),
    )
    .expect("exchange file");
    let state = temporary.path().join("process-state");

    let absent_state = temporary.path().join("absent-operation-state");
    let mut unauthorized = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(&absent_state)
        .arg(&model_path)
        .arg(&exchanges_path)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("unauthorized Proposal Host child");
    unauthorized
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&request).expect("request bytes"))
        .expect("request write");
    let unauthorized_output = unauthorized
        .wait_with_output()
        .expect("unauthorized output");
    assert!(!unauthorized_output.status.success());
    assert!(unauthorized_output.stdout.is_empty());

    let mut forged_value: Value =
        serde_json::from_slice(&cairn_codec::to_vec(&request).expect("request bytes"))
            .expect("request value");
    forged_value["runtime"]["binary_identity"] = json!(format!("sha256:{}", "0".repeat(64)));
    let forged_request: ProposalHostRequestV1 =
        cairn_codec::from_slice(&cairn_codec::to_vec(&forged_value).expect("forged request bytes"))
            .expect("structurally valid forged request");
    let forged_state = temporary.path().join("wrong-binary-state");
    prepare_host_operation(&forged_state, &forged_request);
    let mut forged_child = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(&forged_state)
        .arg(&model_path)
        .arg(&exchanges_path)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wrong-binary Proposal Host child");
    forged_child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&forged_request).expect("forged request bytes"))
        .expect("request write");
    let forged_output = forged_child.wait_with_output().expect("forged output");
    assert!(!forged_output.status.success());
    assert!(forged_output.stdout.is_empty());

    prepare_host_operation(&state, &request);
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(&state)
        .arg(&model_path)
        .arg(&exchanges_path)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Proposal Host child");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&request).expect("request bytes"))
        .expect("request write");
    let output = child.wait_with_output().expect("child output");
    assert!(
        output.status.success(),
        "Proposal Host failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outcome: ProposalHostOutcomeV1 =
        cairn_codec::from_slice(&output.stdout).expect("Host outcome");
    let ProposalHostOutcomeV1::Terminal { terminal } = outcome else {
        panic!("recorded SIR profile unexpectedly requested a Controller experiment")
    };
    terminal
        .validate_against(&request)
        .expect("terminal binding");
    assert!(
        state
            .join(episode_id.to_string())
            .join("events.db")
            .is_file()
    );
    assert!(
        state
            .join(episode_id.to_string())
            .join("terminal.v1.json")
            .is_file()
    );

    // A second process has no recorded transport input. It can only succeed by validating and
    // replaying the canonical Host terminal without another model dispatch.
    let mut restarted = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(&state)
        .arg(&model_path)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("restarted Proposal Host child");
    restarted
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&request).expect("request bytes"))
        .expect("request write");
    let restarted_output = restarted.wait_with_output().expect("restarted output");
    assert!(
        restarted_output.status.success(),
        "restarted Proposal Host failed: {}",
        String::from_utf8_lossy(&restarted_output.stderr)
    );
    assert_eq!(restarted_output.stdout, output.stdout);

    fs::remove_file(state.join(episode_id.to_string()).join("events.db"))
        .expect("remove temporary event store");
    let mut missing_store = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(&state)
        .arg(&model_path)
        .current_dir(repository_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("missing-store Proposal Host child");
    missing_store
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&request).expect("request bytes"))
        .expect("request write");
    let missing_output = missing_store
        .wait_with_output()
        .expect("missing-store output");
    assert!(!missing_output.status.success());
    assert!(missing_output.stdout.is_empty());
}

#[test]
fn child_process_rejects_oversized_ingress_before_model_access() {
    let temporary = tempfile::tempdir().expect("temporary root");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-proposal-host"))
        .arg(temporary.path().join("state"))
        .arg(temporary.path().join("absent-model.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Proposal Host child");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&vec![b'x'; 2 * 1024 * 1024 + 1])
        .expect("oversized write");
    let output = child.wait_with_output().expect("child output");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("byte limit"));
}
