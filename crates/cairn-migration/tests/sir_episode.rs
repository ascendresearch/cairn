use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, DeploymentName, EpisodeBudget,
    EpisodeCompletionReason, EpisodeStepLimit, EpisodeToolOperationLimit, ModelName,
    ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, RecordedExchange, RecordedModelTransport, ResponsesReasoningReplay,
    ScriptedModelTransport, TransportError, recover_agent_episode,
};
use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryRequestV1, SirEpisodeRunError, SirEpisodeRunInput,
    SirObservationId, SirObservedFactV1, SirProposalSubmissionV1, SirReadLineLimit,
    SirSourceLineNumber, SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace, run_sir_episode,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde_json::{Value, json};

fn write_task(root: &Path) {
    fs::create_dir_all(root.join("include")).expect("include directory");
    fs::write(
        root.join("kernel.cu"),
        "extern \"C\" __global__ void transform(const float* input, float* output, float alpha) {\n    int i = blockIdx.x * blockDim.x + threadIdx.x;\n    output[i] = input[i] * alpha;\n}\n",
    )
    .expect("kernel source");
    fs::write(
        root.join("include/transform.h"),
        "int launch_transform(const float* input, float* output, unsigned count, float alpha);\n",
    )
    .expect("header");
}

fn recovery_request_value() -> Value {
    json!({
        "schema_version":1,
        "caller":{
            "schema_version":1,
            "source_entry_point":"launch_transform",
            "arguments":[
                {"index":0,"name":"input","role":"input-buffer","data_type":"f32","shape":{"kind":"unknown-rank"},"valid_domain":"Readable caller-provided binary32 elements."},
                {"index":1,"name":"output","role":"output-buffer","data_type":"f32","shape":{"kind":"unknown-rank"},"valid_domain":"Writable caller-provided binary32 elements."},
                {"index":2,"name":"count","role":"scalar","data_type":"u32","shape":{"kind":"scalar"},"valid_domain":"Number of logical elements."},
                {"index":3,"name":"alpha","role":"scalar","data_type":"f32","shape":{"kind":"scalar"},"valid_domain":"Caller-provided scale."}
            ],
            "error_behaviors":[],
            "claims":[
                {"id":"elementwise-scaling","layer":"algorithm","statement":"Scale each logical input element by alpha.","references":[]}
            ],
            "exclusions":[],
            "unknowns":[
                {"id":"error-behavior","kind":"error-behavior","question":"What errors must the target report?"},
                {"id":"launch-domain","kind":"shape-or-domain","question":"How does the host launch map count to launched threads?"}
            ]
        },
        "target":{
            "soc":{"kind":"selected","soc":"ascend-910b"},
            "toolchain":{"kind":"selected","toolchain":"cann-8"},
            "environment":{"kind":"selected","environment":"linux"}
        },
        "authorized_evidence":[],
        "prior_feedback":{"kind":"no-prior-feedback"}
    })
}

fn recovery_request() -> IntentRecoveryRequestV1 {
    cairn_codec::from_slice(&cairn_codec::to_vec(&recovery_request_value()).expect("request bytes"))
        .expect("strict request")
}

fn valid_submission() -> Value {
    json!({
        "schema_version":1,
        "observed_facts":[
            {"id":"abi-count-parameter","statement":"The public launch declaration includes an unsigned count parameter.","citations":[{"path":"include/transform.h","start_line":1,"end_line":1}]},
            {"id":"kernel-index","statement":"The kernel derives i from CUDA block and thread indices without a count guard.","citations":[{"path":"kernel.cu","start_line":2,"end_line":3}]},
            {"id":"kernel-write","statement":"The kernel writes input[i] multiplied by alpha to output[i].","citations":[{"path":"kernel.cu","start_line":3,"end_line":3}]}
        ],
        "hypotheses":[
            {
                "id":"addressed-elements-only","layer":"observable-contract",
                "claim":"Only the implementation-specific set of launched indices is required to be transformed.",
                "domain":"Indices addressed by the CUDA launch.",
                "supporting_evidence":[{"source":"observed-fact","observation":"kernel-index"}],
                "counter_evidence":[{"source":"caller-claim","claim":"elementwise-scaling"},{"source":"observed-fact","observation":"abi-count-parameter"}]
            },
            {
                "id":"scale-count-elements","layer":"algorithm",
                "claim":"Every logical element in the caller-declared count domain is scaled by alpha.",
                "domain":"Logical indices 0 through count minus one.",
                "supporting_evidence":[{"source":"caller-claim","claim":"elementwise-scaling"},{"source":"observed-fact","observation":"abi-count-parameter"},{"source":"observed-fact","observation":"kernel-write"}],
                "counter_evidence":[{"source":"observed-fact","observation":"kernel-index"}]
            }
        ],
        "conflicts":[{
            "id":"launch-domain-conflict",
            "statement":"The caller declares logical element scaling while the offered kernel alone exposes no count guard or launch coverage.",
            "claims":[{"source":"hypothesis","hypothesis":"addressed-elements-only"},{"source":"hypothesis","hypothesis":"scale-count-elements"}],
            "evidence":[{"source":"caller-claim","claim":"elementwise-scaling"},{"source":"observed-fact","observation":"kernel-index"}]
        }],
        "unknowns":[{
            "id":"host-launch-coverage","kind":"source-behavior",
            "question":"Does the unseen host implementation launch exactly enough threads for count elements?",
            "evidence":[{"source":"observed-fact","observation":"abi-count-parameter"},{"source":"observed-fact","observation":"kernel-index"}]
        }],
        "invariants":[{
            "id":"element-transform",
            "statement":"For every admitted logical index, the output value is the corresponding input multiplied by alpha.",
            "evidence":[{"source":"caller-claim","claim":"elementwise-scaling"},{"source":"observed-fact","observation":"kernel-write"}]
        }],
        "optimization_freedoms":[{
            "id":"thread-decomposition",
            "statement":"The target may change thread decomposition if the admitted element-transform invariant remains true.",
            "protected_invariants":["element-transform"],
            "evidence":[{"source":"observed-fact","observation":"kernel-index"}]
        }],
        "source_dispositions":[{
            "id":"cuda-launch-geometry","observation":"kernel-index","disposition":"unknown-classification",
            "rationale":"The source shows CUDA indexing but the host launch needed to classify its observable effect is absent.",
            "evidence":[{"source":"observed-fact","observation":"abi-count-parameter"},{"source":"observed-fact","observation":"kernel-index"}]
        }],
        "disambiguation_experiments":[{
            "id":"inspect-host-launch",
            "targets":[{"kind":"conflict","conflict":"launch-domain-conflict"},{"kind":"unknown","unknown":"host-launch-coverage"}],
            "plan":"Inspect or execute the exact host launch across counts that are not multiples of the block size.",
            "predictions":["A count-bounded launch or guard supports the logical count domain.","Uncovered or extra observable writes support an implementation-specific addressed-index contract."]
        }]
    })
}

fn responses() -> Vec<Vec<u8>> {
    let read_arguments = serde_json::to_string(
        &json!({"schema_version":1,"path":"kernel.cu","start_line":1,"line_count":20}),
    )
    .expect("read arguments");
    let submit_arguments = serde_json::to_string(&valid_submission()).expect("submit arguments");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"call-read","name":"sir_read_task_artifact","arguments":read_arguments}]})).expect("read response"),
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"call-submit","name":"sir_submit_intent_hypotheses","arguments":submit_arguments}]})).expect("submit response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"msg-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Proposal submitted."}]}]})).expect("yield response"),
    ]
}

fn run_input(episode_id: EpisodeId, task_id: TaskId) -> SirEpisodeRunInput {
    SirEpisodeRunInput {
        task_id,
        recovery_request: recovery_request(),
        episode_id,
        model_configuration: ContentId::derive(b"recorded SIR model configuration")
            .expect("model configuration"),
        selection: ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("deepseek-recorded").expect("model"),
            deployment: DeploymentName::new("local-recorded").expect("deployment"),
            adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
        },
        budget: EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(6).expect("step limit")),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(12)),
            provider_token_limit: None,
            deadline_unix_ms: None,
            external_meter_limits: None,
        },
        max_output_tokens: ModelOutputTokenLimit::new(8_192).expect("output limit"),
        task_limits: SirTaskLimits::default(),
    }
}

fn codec() -> cairn_agent::NativeProtocolCodec {
    cairn_agent::NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
        store: false,
        reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
    })
    .expect("native codec")
}

#[test]
#[allow(clippy::too_many_lines)]
fn full_recovery_contract_is_recorded_restart_safe_and_replayable() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let response_bytes = responses();
    let mut request_bytes = Vec::new();
    let mut response_index = 0_usize;
    let episode_id = EpisodeId::new();
    let task_id = TaskId::new();
    let outcome = {
        let mut scripted = ScriptedModelTransport::new(
            |request: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
                request_bytes.push(request.request_bytes().to_vec());
                let response = response_bytes
                    .get(response_index)
                    .expect("scripted response")
                    .clone();
                response_index += 1;
                Ok(ModelTransportResponse::without_usage(response))
            },
        );
        run_sir_episode(
            &mut events,
            &mut content,
            &mut scripted,
            codec(),
            workspace.clone(),
            run_input(episode_id, task_id),
        )
        .expect("scripted SIR episode")
    };
    assert_eq!(outcome.steps_started(), 3);
    assert_eq!(outcome.proposal().submission().observed_facts().len(), 3);
    assert_eq!(outcome.proposal().submission().hypotheses().len(), 2);
    assert_eq!(outcome.proposal().submission().conflicts().len(), 1);
    assert_eq!(outcome.proposal().submission().unknowns().len(), 1);
    assert_eq!(outcome.proposal().submission().invariants().len(), 1);
    assert_eq!(
        outcome.proposal().recovery_input(),
        outcome.recovery_input()
    );
    assert_eq!(request_bytes.len(), 3);

    let mut invalid_envelope = serde_json::to_value(outcome.proposal()).expect("proposal value");
    invalid_envelope["schema_version"] = json!(2);
    assert!(
        cairn_codec::from_slice::<IntentHypothesisSetProposalV1>(
            &cairn_codec::to_vec(&invalid_envelope).expect("bytes")
        )
        .is_err()
    );
    let initial = String::from_utf8(request_bytes[0].clone()).expect("initial request");
    assert!(initial.contains("elementwise-scaling"));
    assert!(initial.contains("ascend-910b"));
    assert!(initial.contains("kernel.cu"));
    assert!(initial.contains("sir_read_task_artifact"));
    assert!(!initial.contains("output[i] = input[i] * alpha"));
    for forbidden in [
        "D-039",
        "reduce-sum-f32",
        "restricted-partitions",
        "review_receipt_identity",
        "claims.json",
    ] {
        assert!(!initial.contains(forbidden), "leaked {forbidden}");
    }
    assert!(
        String::from_utf8(request_bytes[1].clone())
            .expect("read continuation")
            .contains("output[i] = input[i] * alpha")
    );
    assert!(
        String::from_utf8(request_bytes[2].clone())
            .expect("submit continuation")
            .contains("accepted_proposal")
    );

    drop(events);
    drop(content);
    let mut recovered_content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("recovered content");
    let recovered_events =
        SqliteEventStore::open(state.path().join("events.db")).expect("recovered events");
    let recovered = recover_agent_episode(
        &recovered_events,
        &mut recovered_content,
        &AgentEpisode::new(episode_id).expect("episode"),
    )
    .expect("terminal recovery");
    assert!(matches!(
        recovered,
        AgentEpisodeState::Completed {
            reason: EpisodeCompletionReason::Yielded,
            steps_started: 3
        }
    ));

    let replay_state = tempfile::tempdir().expect("replay state");
    let mut replay_content = SqliteContentStore::open(
        replay_state.path().join("content.db"),
        replay_state.path().join("cas"),
    )
    .expect("replay content");
    let mut replay_events =
        SqliteEventStore::open(replay_state.path().join("events.db")).expect("replay events");
    let exchanges = request_bytes
        .iter()
        .zip(responses())
        .map(|(request, response)| RecordedExchange {
            request_id: ContentId::derive(request).expect("request identity"),
            response_bytes: response,
            usage: None,
        })
        .collect::<Vec<_>>();
    let mut recorded = RecordedModelTransport::new(exchanges);
    let replay = run_sir_episode(
        &mut replay_events,
        &mut replay_content,
        &mut recorded,
        codec(),
        workspace,
        run_input(episode_id, task_id),
    )
    .expect("recorded replay");
    assert_eq!(replay.proposal_id(), outcome.proposal_id());
    assert_eq!(replay.recovery_input(), outcome.recovery_input());
}

#[test]
fn request_and_submission_boundaries_fail_closed() {
    assert!(SirTaskArtifactPath::new("../escape.cu").is_err());
    assert!(SirSourceLineNumber::new(0).is_err());
    assert!(SirReadLineLimit::new(0).is_err());
    assert!(SirObservationId::new("Observation_1").is_err());
    let mut non_v1 = recovery_request_value();
    non_v1["schema_version"] = json!(2);
    assert!(
        cairn_codec::from_slice::<IntentRecoveryRequestV1>(
            &cairn_codec::to_vec(&non_v1).expect("bytes")
        )
        .is_err()
    );
    let mut invalid_role = recovery_request_value();
    invalid_role["caller"]["arguments"][0]["role"] = json!("runtime-handle");
    assert!(
        cairn_codec::from_slice::<IntentRecoveryRequestV1>(
            &cairn_codec::to_vec(&invalid_role).expect("bytes")
        )
        .is_err()
    );

    let mut dangling = valid_submission();
    dangling["hypotheses"][0]["supporting_evidence"][0]["observation"] =
        json!("missing-observation");
    assert!(
        cairn_codec::from_slice::<SirProposalSubmissionV1>(
            &cairn_codec::to_vec(&dangling).expect("bytes")
        )
        .is_err()
    );
    let fact_without_citations =
        cairn_codec::to_vec(&json!({"id":"fact-one","statement":"observable fact","citations":[]}))
            .expect("invalid fact bytes");
    assert!(cairn_codec::from_slice::<SirObservedFactV1>(&fact_without_citations).is_err());
}

#[test]
fn checked_in_live_caller_request_is_strict_current_v1() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let request = serde_json::from_slice::<IntentRecoveryRequestV1>(
        &fs::read(path).expect("checked-in caller request"),
    )
    .expect("strict caller request");
    assert_eq!(request.caller().claims().len(), 5);
}

#[test]
fn episode_rejects_dangling_references_and_out_of_bundle_citations() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let mut invalid = valid_submission();
    invalid["observed_facts"][0]["citations"][0]["start_line"] = json!(99);
    invalid["observed_facts"][0]["citations"][0]["end_line"] = json!(99);
    invalid["hypotheses"][0]["supporting_evidence"][0]["observation"] =
        json!("missing-observation");
    let submit = serde_json::to_string(&invalid).expect("invalid arguments");
    let response = serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"call-invalid-submit","name":"sir_submit_intent_hypotheses","arguments":submit}]})).expect("response");
    let yield_response = serde_json::to_vec(&json!({"output":[{"type":"message","id":"msg-after-rejection","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Unable to submit."}]}]})).expect("yield");
    let mut responses = [response, yield_response].into_iter();
    let mut transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            Ok(ModelTransportResponse::without_usage(
                responses.next().expect("response"),
            ))
        },
    );
    let result = run_sir_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec(),
        workspace,
        run_input(EpisodeId::new(), TaskId::new()),
    );
    assert!(matches!(
        result,
        Err(SirEpisodeRunError::MissingProposal(
            EpisodeCompletionReason::Yielded
        ))
    ));
}
