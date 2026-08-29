use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, DeploymentName, EpisodeBudget,
    EpisodeCompletionReason, EpisodeStepLimit, EpisodeToolOperationLimit, ModelName,
    ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, RecordedExchange, RecordedModelTransport, ResponsesReasoningReplay,
    ScriptedModelTransport, TransportError, recover_agent_episode,
};
use cairn_migration::{
    AdmittedCollectionOracleClaimArtifact, CandidateEpisodeError, CandidateEpisodeRunInput,
    CollectionCandidateProposalSubmissionV1, CollectionCandidateProposalV1,
    CollectionCandidateSearchAuthorityInput, CollectionCandidateSourcePath,
    CollectionOracleAdmissionPublicOutcomeArtifact, CollectionOracleClaimDomainV1,
    CollectionOracleClaimStrengthV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, MigrationIntentContractArtifact,
    PreparedCollectionCandidateSearchInput, SirCallerClaimId, SirCapabilityManifestV1,
    SirResolvedRuntimeModelArtifact, SirTaskLimits, SirTaskWorkspace,
    prepare_collection_candidate_search_input, run_collection_candidate_episode,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde_json::{Value, json};

fn write_task(root: &Path) {
    fs::create_dir_all(root.join("include")).expect("include directory");
    fs::write(
        root.join("compact.cu"),
        "__global__ void compact(const float* input, unsigned count, float threshold, float* output, unsigned* output_count) {\n  unsigned i = blockIdx.x * blockDim.x + threadIdx.x;\n  if (i < count && input[i] > threshold) {\n    unsigned slot = atomicAdd(output_count, 1U);\n    output[slot] = input[i];\n  }\n}\n",
    )
    .expect("CUDA source");
    fs::write(
        root.join("include/compact.h"),
        "int launch_compact(const float* input, unsigned count, float threshold, float* output, unsigned* output_count, void* stream);\n",
    )
    .expect("header");
}

fn recovery_request_value() -> Value {
    json!({
        "schema_version":1,
        "caller":{
            "schema_version":1,
            "source_entry_point":"launch_compact",
            "arguments":[
                {"index":0,"name":"input","role":"input-buffer","data_type":"f32","shape":{"kind":"ranked","dimensions":["count"]},"valid_domain":"Readable finite normal binary32 elements."},
                {"index":1,"name":"count","role":"scalar","data_type":"u32","shape":{"kind":"scalar"},"valid_domain":"Logical input length and output capacity."},
                {"index":2,"name":"threshold","role":"scalar","data_type":"f32","shape":{"kind":"scalar"},"valid_domain":"Finite normal binary32 threshold."},
                {"index":3,"name":"output","role":"output-buffer","data_type":"f32","shape":{"kind":"ranked","dimensions":["count"]},"valid_domain":"Writable count-element output."},
                {"index":4,"name":"output_count","role":"output-buffer","data_type":"u32","shape":{"kind":"ranked","dimensions":["1"]},"valid_domain":"Writable one-element count."},
                {"index":5,"name":"stream","role":"runtime-handle","data_type":null,"shape":{"kind":"opaque-handle"},"valid_domain":null}
            ],
            "error_behaviors":["Return target runtime status without host synchronization."],
            "claims":[
                {"id":"copies-strictly-above","layer":"algorithm","statement":"Copy every input occurrence strictly greater than threshold.","references":[]},
                {"id":"reported-count","layer":"observable-contract","statement":"Report the exact selected occurrence count.","references":[]}
            ],
            "exclusions":[],
            "unknowns":[
                {"id":"output-order","kind":"observable-contract","question":"Is output order observable?"}
            ]
        },
        "target":{
            "soc":{"kind":"not-selected"},
            "toolchain":{"kind":"not-selected"},
            "environment":{"kind":"not-selected"}
        },
        "authorized_evidence":[],
        "prior_feedback":{"kind":"no-prior-feedback"}
    })
}

fn recovery_request() -> IntentRecoveryRequestV1 {
    cairn_codec::from_slice(
        &cairn_codec::to_vec(&recovery_request_value()).expect("recovery request bytes"),
    )
    .expect("strict recovery request")
}

fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
    ContentId::derive(label).expect("content identity")
}

fn authority_input(
    task_id: TaskId,
    recovery_input: ContentId<IntentRecoveryInputArtifact>,
) -> CollectionCandidateSearchAuthorityInput {
    CollectionCandidateSearchAuthorityInput::new(
        task_id,
        recovery_input,
        id::<MigrationIntentContractArtifact>(b"admitted intent contract"),
        id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"published local Oracle"),
        id::<AdmittedCollectionOracleClaimArtifact>(b"local Oracle claim"),
        SirCallerClaimId::new("copies-strictly-above").expect("selection claim"),
        CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
        CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
    )
}

fn candidate_inputs(
    workspace: &SirTaskWorkspace,
    task_id: TaskId,
) -> (
    IntentRecoveryInputV1,
    PreparedCollectionCandidateSearchInput,
) {
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        workspace.bundle().identity().expect("task bundle identity"),
        recovery_request(),
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery input");
    let search = prepare_collection_candidate_search_input(&authority_input(
        task_id,
        recovery.identity().expect("recovery identity"),
    ))
    .expect("Candidate search input");
    (recovery, search)
}

fn proposal_submission() -> Value {
    json!({
        "schema_version":1,
        "files":[
            {
                "path":"src/compact_above.cpp",
                "source":"#include \"kernel_operator.h\"\n\nextern \"C\" __global__ __aicore__ void compact_above() {\n  // Initial unbuilt Candidate proposal for the admitted local semantics.\n}\n"
            }
        ],
        "primary_source":"src/compact_above.cpp",
        "explanation":"Maps the qualifying-occurrence compaction contract to one Ascend C kernel proposal. Target-specific tiling and runtime launch details remain unresolved until a target is selected."
    })
}

fn responses() -> Vec<Vec<u8>> {
    let read_arguments = serde_json::to_string(
        &json!({"schema_version":1,"path":"compact.cu","start_line":1,"line_count":20}),
    )
    .expect("read arguments");
    let submit_arguments = serde_json::to_string(&proposal_submission()).expect("submit arguments");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"candidate-read","name":"candidate_read_task_artifact","arguments":read_arguments}]})).expect("read response"),
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"candidate-submit","name":"candidate_submit_collection_proposal","arguments":submit_arguments}]})).expect("submit response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"candidate-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Candidate proposal submitted."}]}]})).expect("yield response"),
    ]
}

fn codec() -> cairn_agent::NativeProtocolCodec {
    cairn_agent::NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
        store: false,
        reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
    })
    .expect("native codec")
}

fn run_input(
    episode_id: EpisodeId,
    recovery_input: IntentRecoveryInputV1,
    search_input: PreparedCollectionCandidateSearchInput,
) -> CandidateEpisodeRunInput {
    CandidateEpisodeRunInput {
        search_input,
        recovery_input,
        episode_id,
        model_configuration: id::<SirResolvedRuntimeModelArtifact>(
            b"recorded Candidate model configuration",
        ),
        selection: ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("deepseek-candidate-recorded").expect("model"),
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
        max_output_tokens: ModelOutputTokenLimit::new(16_384).expect("output limit"),
        task_limits: SirTaskLimits::default(),
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn candidate_proposal_is_recorded_answer_free_restart_safe_and_replayable() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let task_id = TaskId::new();
    let episode_id = EpisodeId::new();
    let (recovery, search) = candidate_inputs(&workspace, task_id);
    let search_id = search.id();
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let response_bytes = responses();
    let mut request_bytes = Vec::new();
    let mut response_index = 0_usize;
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
        run_collection_candidate_episode(
            &mut events,
            &mut content,
            &mut scripted,
            codec(),
            workspace.clone(),
            run_input(episode_id, recovery.clone(), search.clone()),
        )
        .expect("scripted Candidate episode")
    };
    assert_eq!(outcome.steps_started(), 3);
    assert_eq!(outcome.search_input(), search_id);
    assert_eq!(outcome.proposal().search_input(), search_id);
    assert_eq!(outcome.proposal().episode_id(), episode_id);
    assert_eq!(outcome.proposal().submission().files().len(), 1);
    assert_eq!(
        outcome.proposal().submission().primary_source().as_str(),
        "src/compact_above.cpp"
    );
    assert_eq!(request_bytes.len(), 3);

    let initial = String::from_utf8(request_bytes[0].clone()).expect("initial request");
    assert!(initial.contains("copies-strictly-above"));
    assert!(initial.contains("exact-occurrence-multiset-and-reported-count"));
    assert!(initial.contains("compact.cu"));
    assert!(initial.contains("candidate_read_task_artifact"));
    assert!(!initial.contains("atomicAdd(output_count"));
    for forbidden in [
        "qualification_receipt",
        "comparison_evidence",
        "execution_receipt",
        "missing_occurrence",
        "honest_reordered",
        "expected_collection",
    ] {
        assert!(!initial.contains(forbidden), "leaked {forbidden}");
    }
    assert!(
        String::from_utf8(request_bytes[1].clone())
            .expect("read continuation")
            .contains("atomicAdd(output_count")
    );
    assert!(
        String::from_utf8(request_bytes[2].clone())
            .expect("submit continuation")
            .contains("accepted_candidate_proposal")
    );

    let proposal_bytes = cairn_codec::to_vec(outcome.proposal()).expect("proposal bytes");
    let decoded: CollectionCandidateProposalV1 =
        cairn_codec::from_slice(&proposal_bytes).expect("strict proposal round trip");
    assert_eq!(decoded, *outcome.proposal());
    assert_eq!(
        decoded.identity().expect("proposal identity"),
        outcome.proposal_id()
    );

    drop(events);
    drop(content);
    let mut recovered_content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("recovered content");
    let recovered_events =
        SqliteEventStore::open(state.path().join("events.db")).expect("recovered events");
    assert!(matches!(
        recover_agent_episode(
            &recovered_events,
            &mut recovered_content,
            &AgentEpisode::new(episode_id).expect("episode")
        )
        .expect("terminal recovery"),
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
    let replay = run_collection_candidate_episode(
        &mut replay_events,
        &mut replay_content,
        &mut recorded,
        codec(),
        workspace,
        run_input(episode_id, recovery, search),
    )
    .expect("recorded Candidate replay");
    assert_eq!(replay.proposal_id(), outcome.proposal_id());
    assert_eq!(replay.search_input(), outcome.search_input());
}

#[test]
fn candidate_submission_and_authority_bindings_fail_closed() {
    assert!(CollectionCandidateSourcePath::new("../escape.cpp").is_err());
    let mut invalid = proposal_submission();
    invalid["schema_version"] = json!(2);
    assert!(decode_submission(&invalid).is_err());
    let mut invalid = proposal_submission();
    invalid["legacy_verdict"] = json!("pass");
    assert!(decode_submission(&invalid).is_err());
    let mut invalid = proposal_submission();
    invalid["primary_source"] = json!("src/missing.cpp");
    assert!(decode_submission(&invalid).is_err());
    let mut invalid = proposal_submission();
    invalid["files"][0]["source"] = json!("");
    assert!(decode_submission(&invalid).is_err());
    let mut invalid = proposal_submission();
    invalid["files"][0]["source"] = json!("x".repeat(128 * 1024 + 1));
    assert!(decode_submission(&invalid).is_err());
    let mut invalid = proposal_submission();
    invalid["files"] = json!([
        {"path":"z.cpp","source":"z"},
        {"path":"a.cpp","source":"a"}
    ]);
    invalid["primary_source"] = json!("a.cpp");
    assert!(decode_submission(&invalid).is_err());

    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let task_id = TaskId::new();
    let (recovery, _) = candidate_inputs(&workspace, task_id);
    let changed_search = prepare_collection_candidate_search_input(&authority_input(
        task_id,
        id::<IntentRecoveryInputArtifact>(b"different recovery input"),
    ))
    .expect("changed search input");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let mut called = false;
    let mut transport = ScriptedModelTransport::new(
        |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            called = true;
            Err(TransportError::Rejected("must not dispatch".to_owned()))
        },
    );
    let result = run_collection_candidate_episode(
        &mut events,
        &mut content,
        &mut transport,
        codec(),
        workspace,
        run_input(EpisodeId::new(), recovery, changed_search),
    );
    assert!(matches!(
        result,
        Err(CandidateEpisodeError::InvalidStructure(
            "Candidate task/recovery/search binding"
        ))
    ));
    assert!(!called, "authority mismatch reached the model transport");
}

#[test]
fn accepted_proposal_still_requires_an_explicit_model_yield() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let (recovery, search) = candidate_inputs(&workspace, TaskId::new());
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let response_bytes = responses();
    let mut response_index = 0_usize;
    let mut transport = ScriptedModelTransport::new(
        |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            let response = response_bytes
                .get(response_index)
                .expect("scripted response")
                .clone();
            response_index += 1;
            Ok(ModelTransportResponse::without_usage(response))
        },
    );
    let mut input = run_input(EpisodeId::new(), recovery, search);
    input.budget.step_limit = Some(EpisodeStepLimit::new(2).expect("step limit"));
    assert!(matches!(
        run_collection_candidate_episode(
            &mut events,
            &mut content,
            &mut transport,
            codec(),
            workspace,
            input,
        ),
        Err(CandidateEpisodeError::ProposalNotYielded(
            EpisodeCompletionReason::StepLimitReached
        ))
    ));
    assert_eq!(
        response_index, 2,
        "budget completion dispatched another step"
    );
}

fn decode_submission(value: &Value) -> Result<CollectionCandidateProposalSubmissionV1, String> {
    cairn_codec::from_slice(&cairn_codec::to_vec(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
