use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, DeploymentName, EpisodeBudget,
    EpisodeCompletionReason, EpisodeStepLimit, EpisodeToolOperationLimit, ModelName,
    ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, RecordedExchange, RecordedModelTransport, ResponsesReasoningReplay,
    ScriptedModelTransport, TransportError, recover_agent_episode,
};
use cairn_migration::{
    IntentHypothesisSetProposalV1, SirCitedFactV1, SirEpisodeRunError, SirEpisodeRunInput,
    SirHypothesisSummary, SirProposalSubmissionV1, SirReadLineLimit, SirSourceLineNumber,
    SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace, run_sir_episode,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde_json::json;

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

fn responses() -> Vec<Vec<u8>> {
    let read_arguments = serde_json::to_string(&json!({
        "schema_version":1,
        "path":"kernel.cu",
        "start_line":1,
        "line_count":20
    }))
    .expect("read arguments");
    let submit_arguments = serde_json::to_string(&json!({
        "schema_version":1,
        "hypotheses":[
            {
                "summary":"The operation scales every addressed input element by the caller-provided alpha value.",
                "supporting_facts":[{
                    "statement":"The kernel writes input[i] multiplied by alpha to output[i].",
                    "citations":[{"path":"kernel.cu","start_line":3,"end_line":3}]
                }],
                "counter_facts":[]
            },
            {
                "summary":"The observable contract may be limited to an implementation-specific subset of launched indices.",
                "supporting_facts":[{
                    "statement":"The source computes an index from CUDA launch geometry without an in-kernel count check.",
                    "citations":[{"path":"kernel.cu","start_line":2,"end_line":3}]
                }],
                "counter_facts":[{
                    "statement":"The public ABI includes a count parameter in the host declaration.",
                    "citations":[{"path":"include/transform.h","start_line":1,"end_line":1}]
                }]
            }
        ],
        "unknowns":[{
            "question":"Does the unseen host launch guarantee that every launched index is smaller than count?",
            "citations":[{"path":"kernel.cu","start_line":2,"end_line":3}]
        }]
    }))
    .expect("submission arguments");
    vec![
        serde_json::to_vec(&json!({
            "output":[{
                "type":"function_call",
                "call_id":"call-read",
                "name":"sir_read_task_artifact",
                "arguments":read_arguments
            }]
        }))
        .expect("read response"),
        serde_json::to_vec(&json!({
            "output":[{
                "type":"function_call",
                "call_id":"call-submit",
                "name":"sir_submit_intent_hypotheses",
                "arguments":submit_arguments
            }]
        }))
        .expect("submit response"),
        serde_json::to_vec(&json!({
            "output":[{
                "type":"message",
                "id":"msg-final",
                "phase":"final_answer",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":"Proposal submitted."}]
            }]
        }))
        .expect("yield response"),
    ]
}

fn run_input(episode_id: EpisodeId) -> SirEpisodeRunInput {
    SirEpisodeRunInput {
        task_id: TaskId::new(),
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
        max_output_tokens: ModelOutputTokenLimit::new(4_096).expect("output limit"),
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
#[allow(clippy::too_many_lines)] // Keep the exact request, restart, and replay proof together.
fn scripted_episode_and_recorded_replay_share_exact_requests_without_fixture_answers() {
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
            run_input(episode_id),
        )
        .expect("scripted SIR episode")
    };
    assert_eq!(outcome.steps_started(), 3);
    assert_eq!(outcome.proposal().submission().hypotheses().len(), 2);
    assert_eq!(outcome.proposal().submission().unknowns().len(), 1);
    assert_eq!(request_bytes.len(), 3);
    for artifact in workspace.bundle().artifacts() {
        let mut archived_source = Vec::new();
        content
            .write_to(&artifact.identity(), &mut archived_source)
            .expect("archived task artifact");
        assert!(!archived_source.is_empty());
    }
    let mut invalid_envelope = serde_json::to_value(outcome.proposal()).expect("proposal value");
    invalid_envelope["schema_version"] = json!(2);
    assert!(
        cairn_codec::from_slice::<IntentHypothesisSetProposalV1>(
            &cairn_codec::to_vec(&invalid_envelope).expect("invalid envelope bytes")
        )
        .is_err()
    );

    let initial = String::from_utf8(request_bytes[0].clone()).expect("initial request");
    assert!(initial.contains("kernel.cu"));
    assert!(initial.contains("sir_read_task_artifact"));
    assert!(!initial.contains("output[i] = input[i] * alpha"));
    for forbidden in [
        "D-039",
        "reduce-sum-f32",
        "restricted-partitions",
        "review_receipt_identity",
        "public-corpus.json",
        "claims.json",
    ] {
        assert!(!initial.contains(forbidden), "leaked {forbidden}");
    }
    let after_read = String::from_utf8(request_bytes[1].clone()).expect("read continuation");
    assert!(after_read.contains("output[i] = input[i] * alpha"));
    let after_submit = String::from_utf8(request_bytes[2].clone()).expect("submit continuation");
    assert!(after_submit.contains("accepted_proposal"));

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
    .expect("terminal episode recovery");
    assert!(matches!(
        recovered,
        AgentEpisodeState::Completed {
            reason: EpisodeCompletionReason::Yielded,
            steps_started: 3,
        }
    ));
    drop(recovered_events);
    drop(recovered_content);

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
        run_input(episode_id),
    )
    .expect("recorded replay");
    assert_eq!(replay.proposal_id(), outcome.proposal_id());
    assert_eq!(replay.task_bundle(), outcome.task_bundle());
}

#[test]
fn task_and_submission_boundaries_fail_closed() {
    assert!(SirTaskArtifactPath::new("../escape.cu").is_err());
    assert!(SirTaskArtifactPath::new("/absolute.cu").is_err());
    assert!(SirTaskArtifactPath::new("nested\\host.cu").is_err());
    assert!(SirSourceLineNumber::new(0).is_err());
    assert!(SirReadLineLimit::new(0).is_err());
    assert!(SirHypothesisSummary::new("  padded  ").is_err());

    let one_hypothesis = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "hypotheses":[{
            "summary":"only one",
            "supporting_facts":[{
                "statement":"fact",
                "citations":[{"path":"kernel.cu","start_line":1,"end_line":1}]
            }],
            "counter_facts":[]
        }],
        "unknowns":[{"question":"unknown","citations":[]}]
    }))
    .expect("one hypothesis bytes");
    assert!(cairn_codec::from_slice::<SirProposalSubmissionV1>(&one_hypothesis).is_err());

    let fact_without_citations = cairn_codec::to_vec(&json!({
        "statement":"observable fact",
        "citations":[]
    }))
    .expect("invalid fact bytes");
    assert!(cairn_codec::from_slice::<SirCitedFactV1>(&fact_without_citations).is_err());
}

#[test]
fn episode_rejects_citations_outside_the_frozen_bundle() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let invalid = serde_json::to_string(&json!({
        "schema_version":1,
        "hypotheses":[
            {
                "summary":"first",
                "supporting_facts":[{
                    "statement":"outside line",
                    "citations":[{"path":"kernel.cu","start_line":99,"end_line":99}]
                }],
                "counter_facts":[]
            },
            {
                "summary":"second",
                "supporting_facts":[{
                    "statement":"valid shape",
                    "citations":[{"path":"kernel.cu","start_line":1,"end_line":1}]
                }],
                "counter_facts":[]
            }
        ],
        "unknowns":[{"question":"unknown","citations":[]}]
    }))
    .expect("invalid submission arguments");
    let response = serde_json::to_vec(&json!({
        "output":[{
            "type":"function_call",
            "call_id":"call-invalid-submit",
            "name":"sir_submit_intent_hypotheses",
            "arguments":invalid
        }]
    }))
    .expect("invalid submission response");
    let yield_response = serde_json::to_vec(&json!({
        "output":[{
            "type":"message",
            "id":"msg-after-rejection",
            "phase":"final_answer",
            "role":"assistant",
            "status":"completed",
            "content":[{"type":"output_text","text":"Unable to submit."}]
        }]
    }))
    .expect("yield response");
    let mut responses = [response, yield_response].into_iter();
    let mut transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            Ok(ModelTransportResponse::without_usage(
                responses.next().expect("scripted response"),
            ))
        },
    );
    let result = run_sir_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec(),
        workspace,
        run_input(EpisodeId::new()),
    );
    assert!(matches!(
        result,
        Err(SirEpisodeRunError::MissingProposal(
            EpisodeCompletionReason::Yielded
        ))
    ));
}
