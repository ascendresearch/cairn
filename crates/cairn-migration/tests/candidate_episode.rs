use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, AgentEpisode, AgentEpisodeState, DeploymentName, EpisodeBudget,
    EpisodeCompletionReason, EpisodeStepLimit, EpisodeToolOperationLimit, ModelName,
    ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, RecordedExchange, RecordedModelTransport, ResponsesReasoningReplay,
    ScriptedModelTransport, TransportError, recover_agent_episode,
};
use cairn_execution::{
    DOCKER_BACKEND, DockerImageId, ExecutionBackend, ExecutionEvidenceArtifact,
    ExecutionObservation, ExecutionReceipt, ExecutionReceiptArtifact, ExecutionStderrArtifact,
    ExecutionStdoutArtifact, ResolvedProgramIdentity, TrustedExecutionEvidence,
};
use cairn_migration::{
    AdmittedCollectionOracleClaimArtifact, CandidateBuildEnvironmentProfileV1,
    CandidateEpisodeError, CandidateEpisodeRunInput, CandidateNativeFollowupEpisodeRunInput,
    CandidateRevisionEpisodeRunInput, CollectionCandidateProposalSubmissionV1,
    CollectionCandidateProposalV1, CollectionCandidateSearchAuthorityInput,
    CollectionCandidateSourcePath, CollectionOracleAdmissionPublicOutcomeArtifact,
    CollectionOracleClaimDomainV1, CollectionOracleClaimStrengthV1, IntentRecoveryInputArtifact,
    IntentRecoveryInputV1, IntentRecoveryRequestV1, MigrationIntentContractArtifact,
    PreparedCandidateBuildDiagnostic, PreparedCandidateNativeBuildDiagnostic,
    PreparedCollectionCandidateSearchInput, SirCallerClaimId, SirCapabilityManifestV1,
    SirResolvedRuntimeModelArtifact, SirTaskLimits, SirTaskWorkspace,
    prepare_candidate_build_diagnostic, prepare_candidate_build_job,
    prepare_candidate_native_build_diagnostic, prepare_candidate_native_revision_build_job,
    prepare_collection_candidate_search_input, run_collection_candidate_episode,
    run_collection_candidate_native_followup_episode, run_collection_candidate_revision_episode,
};
use cairn_protocol::{AttemptId, ContentId, ContentType, EpisodeId, JobId, TaskId};
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

fn revision_submission() -> Value {
    json!({
        "schema_version":1,
        "files":[
            {
                "path":"CMakeLists.txt",
                "source":"cmake_minimum_required(VERSION 3.24)\nfind_package(ASC REQUIRED)\nproject(candidate LANGUAGES ASC CXX)\nadd_library(candidate STATIC src/compact_above.asc)\ntarget_compile_options(candidate PRIVATE $<$<COMPILE_LANGUAGE:ASC>:--npu-arch=dav-3510>)\n"
            },
            {
                "path":"src/compact_above.asc",
                "source":"#include \"kernel_operator.h\"\n\nextern \"C\" __global__ __aicore__ void compact_above() {\n  // Revised complete source after the public compiler diagnostic.\n}\n"
            }
        ],
        "primary_source":"src/compact_above.asc",
        "explanation":"Switches the kernel translation unit to the selected Ascend compiler integration while preserving the frozen local collection contract. This remains an unbuilt proposal."
    })
}

fn revision_responses() -> Vec<Vec<u8>> {
    let submit_arguments =
        serde_json::to_string(&revision_submission()).expect("revision arguments");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"candidate-revision-submit","name":"candidate_submit_collection_revision","arguments":submit_arguments}]})).expect("revision submit response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"candidate-revision-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Candidate revision submitted."}]}]})).expect("revision yield response"),
    ]
}

fn native_followup_submission() -> Value {
    json!({
        "schema_version":1,
        "files":[
            {"path":"include/compact_above.h","source":"#pragma once\n#include <acl/acl.h>\nextern \"C\" aclError launch_compact_above_f32();\n"},
            {"path":"src/compact_above.asc","source":"#include \"kernel_operator.h\"\nusing namespace AscendC;\nextern \"C\" __global__ __aicore__ void compact_above_kernel(GM_ADDR input) { (void)input; }\n"}
        ],
        "primary_source":"src/compact_above.asc",
        "explanation":"Separates the native ASC translation unit from host launch code in response to the exact bisheng diagnostic. This remains unbuilt."
    })
}

fn native_followup_responses() -> Vec<Vec<u8>> {
    let arguments =
        serde_json::to_string(&native_followup_submission()).expect("follow-up arguments");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"native-followup-submit","name":"candidate_submit_native_followup_revision","arguments":arguments}]})).expect("follow-up submit response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"native-followup-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Native follow-up submitted."}]}]})).expect("follow-up yield response"),
    ]
}

struct RevisionFixture {
    parent: CollectionCandidateProposalV1,
    parent_id: cairn_protocol::ContentId<cairn_migration::CollectionCandidateProposalArtifact>,
    diagnostic: PreparedCandidateBuildDiagnostic,
}

struct NativeFollowupFixture {
    previous: cairn_migration::CollectionCandidateRevisionV1,
    previous_id: ContentId<cairn_migration::CollectionCandidateRevisionArtifact>,
    diagnostic: PreparedCandidateNativeBuildDiagnostic,
}

fn revision_fixture(
    search_input: cairn_protocol::ContentId<
        cairn_migration::CollectionCandidateSearchInputArtifact,
    >,
) -> RevisionFixture {
    let parent_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "search_input":search_input,
        "episode_id":EpisodeId::new(),
        "model_configuration":id::<SirResolvedRuntimeModelArtifact>(b"parent model"),
        "submission":{
            "schema_version":1,
            "files":[
                {"path":"CMakeLists.txt","source":"cmake_minimum_required(VERSION 3.24)\nproject(candidate LANGUAGES CXX)\nadd_library(candidate STATIC src/compact_above.cpp)\ntarget_link_libraries(candidate PRIVATE ascendcl)\n"},
                {"path":"src/compact_above.cpp","source":"#include <acl/acl.h>\n#include \"kernel_operator.h\"\n\nextern \"C\" __global__ __aicore__ void compact_above() {}\n"}
            ],
            "primary_source":"src/compact_above.cpp",
            "explanation":"Initial proposal with an unresolved target build integration."
        }
    })).expect("parent bytes");
    let parent_id = ContentId::derive(&parent_bytes).expect("parent ID");
    let parent: CollectionCandidateProposalV1 =
        cairn_codec::from_slice(&parent_bytes).expect("parent proposal");
    let job_id = JobId::new();
    let build = prepare_candidate_build_job(
        job_id,
        &parent_bytes,
        parent_id,
        DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
    )
    .expect("Candidate build");
    let stderr = b"src/compact_above.cpp:1:10: fatal error: acl/acl.h: No such file or directory\n";
    let stderr_id = ContentId::<ExecutionStderrArtifact>::derive(stderr).expect("stderr ID");
    let evidence_value = TrustedExecutionEvidence::new(
        ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
        build.environment_id(),
        ResolvedProgramIdentity::new("sha256:recorded-candidate-build").expect("program"),
        vec![ExecutionObservation::new("docker:accelerator:none").expect("observation")],
    )
    .expect("evidence");
    let evidence = cairn_codec::to_vec(&evidence_value).expect("evidence bytes");
    let evidence_id =
        ContentId::<ExecutionEvidenceArtifact>::derive(&evidence).expect("evidence ID");
    let receipt_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "job_id":job_id,
        "attempt_id":AttemptId::new(),
        "contract_id":build.contract_id(),
        "outcome":"subject-failed",
        "exit_code":1,
        "elapsed_ms":10,
        "stdout_id":id::<ExecutionStdoutArtifact>(b"stdout"),
        "stderr_id":stderr_id,
        "evidence_id":evidence_id,
        "outputs":[]
    }))
    .expect("receipt bytes");
    let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
    let receipt_id =
        ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).expect("receipt ID");
    let diagnostic =
        prepare_candidate_build_diagnostic(&build, receipt_id, &receipt, stderr, &evidence)
            .expect("build diagnostic");
    RevisionFixture {
        parent,
        parent_id,
        diagnostic,
    }
}

fn native_followup_fixture(
    search_input: ContentId<cairn_migration::CollectionCandidateSearchInputArtifact>,
) -> NativeFollowupFixture {
    let previous_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "search_input":search_input,
        "parent_proposal":id::<cairn_migration::CollectionCandidateProposalArtifact>(b"parent proposal"),
        "build_diagnostic":id::<cairn_migration::CollectionCandidateBuildDiagnosticArtifact>(b"generic diagnostic"),
        "episode_id":EpisodeId::new(),
        "model_configuration":id::<SirResolvedRuntimeModelArtifact>(b"previous model"),
        "submission":{
            "schema_version":1,
            "files":[
                {"path":"CMakeLists.txt","source":"project(previous LANGUAGES CXX)\nadd_library(previous STATIC src/compact.cpp)\n"},
                {"path":"src/compact.cpp","source":"#include \"kernel_operator.h\"\nusing namespace AscendC;\nclass Kernel { public: __aicore__ Kernel() {} };\nvoid host() { Kernel kernel; }\n"}
            ],
            "primary_source":"src/compact.cpp",
            "explanation":"Previous source before the native compiler feedback."
        }
    })).expect("previous revision bytes");
    let previous_id = ContentId::derive(&previous_bytes).expect("previous revision ID");
    let previous: cairn_migration::CollectionCandidateRevisionV1 =
        cairn_codec::from_slice(&previous_bytes).expect("previous revision");
    let job_id = JobId::new();
    let build = prepare_candidate_native_revision_build_job(
        job_id,
        &previous_bytes,
        previous_id,
        DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
    )
    .expect("native build");
    let stderr =
        b"candidate_primary.asc:4: error: call to __aicore__ function from __host__ function\n";
    let stderr_id = ContentId::<ExecutionStderrArtifact>::derive(stderr).expect("stderr ID");
    let evidence_value = TrustedExecutionEvidence::new(
        ExecutionBackend::new(DOCKER_BACKEND).expect("backend"),
        build.environment_id(),
        ResolvedProgramIdentity::new("sha256:native-gate").expect("program"),
        vec![ExecutionObservation::new("docker:accelerator:none").expect("observation")],
    )
    .expect("evidence");
    let evidence = cairn_codec::to_vec(&evidence_value).expect("evidence bytes");
    let evidence_id =
        ContentId::<ExecutionEvidenceArtifact>::derive(&evidence).expect("evidence ID");
    let receipt_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "job_id":job_id,
        "attempt_id":AttemptId::new(),
        "contract_id":build.contract_id(),
        "outcome":"subject-failed",
        "exit_code":1,
        "elapsed_ms":12,
        "stdout_id":id::<ExecutionStdoutArtifact>(b"native stdout"),
        "stderr_id":stderr_id,
        "evidence_id":evidence_id,
        "outputs":[]
    }))
    .expect("native receipt bytes");
    let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
    let receipt_id = ContentId::derive(&receipt_bytes).expect("receipt ID");
    let diagnostic =
        prepare_candidate_native_build_diagnostic(&build, receipt_id, &receipt, stderr, &evidence)
            .expect("native diagnostic");
    NativeFollowupFixture {
        previous,
        previous_id,
        diagnostic,
    }
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

#[test]
#[allow(clippy::too_many_lines)]
fn receipt_bound_revision_is_isolated_restart_safe_and_replayable() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let (recovery, search) = candidate_inputs(&workspace, TaskId::new());
    let fixture = revision_fixture(search.id());
    let parent_episode = fixture.parent.episode_id();
    let revision_episode = EpisodeId::new();
    assert_ne!(revision_episode, parent_episode);
    let model_configuration =
        id::<SirResolvedRuntimeModelArtifact>(b"recorded revision model configuration");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let response_bytes = revision_responses();
    let mut request_bytes = Vec::new();
    let mut response_index = 0_usize;
    let run_input = |episode_id| CandidateRevisionEpisodeRunInput {
        search_input: search.clone(),
        recovery_input: recovery.clone(),
        parent: fixture.parent.clone(),
        parent_id: fixture.parent_id,
        build_diagnostic: fixture.diagnostic.clone(),
        episode_id,
        model_configuration,
        selection: ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("deepseek-candidate-revision-recorded").expect("model"),
            deployment: DeploymentName::new("local-recorded").expect("deployment"),
            adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
        },
        budget: EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(4).expect("step limit")),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(8)),
            provider_token_limit: None,
            deadline_unix_ms: None,
            external_meter_limits: None,
        },
        max_output_tokens: ModelOutputTokenLimit::new(16_384).expect("output limit"),
        task_limits: SirTaskLimits::default(),
    };
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
        run_collection_candidate_revision_episode(
            &mut events,
            &mut content,
            &mut scripted,
            codec(),
            workspace.clone(),
            run_input(revision_episode),
        )
        .expect("scripted Candidate revision episode")
    };
    assert_eq!(outcome.steps_started(), 2);
    assert_eq!(outcome.parent_id(), fixture.parent_id);
    assert_eq!(outcome.diagnostic_id(), fixture.diagnostic.id());
    assert_eq!(outcome.revision().episode_id(), revision_episode);
    assert_eq!(
        outcome.revision().model_configuration(),
        model_configuration
    );
    assert_eq!(outcome.revision().parent_proposal(), fixture.parent_id);
    assert_eq!(
        outcome.revision().build_diagnostic(),
        fixture.diagnostic.id()
    );
    assert_ne!(outcome.revision().submission(), fixture.parent.submission());
    assert_eq!(request_bytes.len(), 2);

    let initial = String::from_utf8(request_bytes[0].clone()).expect("initial request");
    assert!(initial.contains("candidate_submit_collection_revision"));
    assert!(initial.contains("#include <acl/acl.h>"));
    assert!(initial.contains("fatal error: acl/acl.h: No such file or directory"));
    assert!(initial.contains(&fixture.parent_id.to_string()));
    assert!(initial.contains(&fixture.diagnostic.id().to_string()));
    assert!(!initial.contains("accepted_candidate_proposal"));
    for forbidden in [
        "qualification_receipt",
        "comparison_evidence",
        "missing_occurrence",
        "expected_collection",
        "private_continuation",
    ] {
        assert!(!initial.contains(forbidden), "leaked {forbidden}");
    }
    let continuation = String::from_utf8(request_bytes[1].clone()).expect("revision continuation");
    assert!(continuation.contains("accepted_candidate_revision"));
    assert!(continuation.contains(&outcome.revision_id().to_string()));

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
            &AgentEpisode::new(revision_episode).expect("episode")
        )
        .expect("terminal recovery"),
        AgentEpisodeState::Completed {
            reason: EpisodeCompletionReason::Yielded,
            steps_started: 2
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
        .zip(revision_responses())
        .map(|(request, response)| RecordedExchange {
            request_id: ContentId::derive(request).expect("request identity"),
            response_bytes: response,
            usage: None,
        })
        .collect::<Vec<_>>();
    let mut recorded = RecordedModelTransport::new(exchanges);
    let replay = run_collection_candidate_revision_episode(
        &mut replay_events,
        &mut replay_content,
        &mut recorded,
        codec(),
        workspace,
        run_input(revision_episode),
    )
    .expect("recorded Candidate revision replay");
    assert_eq!(replay.revision_id(), outcome.revision_id());
    assert_eq!(replay.parent_id(), outcome.parent_id());
    assert_eq!(replay.diagnostic_id(), outcome.diagnostic_id());
}

#[test]
#[allow(clippy::too_many_lines)]
fn native_compiler_followup_is_isolated_restart_safe_and_replayable() {
    let task = tempfile::tempdir().expect("task root");
    write_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let (recovery, search) = candidate_inputs(&workspace, TaskId::new());
    let fixture = native_followup_fixture(search.id());
    let previous_episode = fixture.previous.episode_id();
    let followup_episode = EpisodeId::new();
    assert_ne!(followup_episode, previous_episode);
    let model_configuration =
        id::<SirResolvedRuntimeModelArtifact>(b"recorded native follow-up model configuration");
    let state = tempfile::tempdir().expect("state root");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content store");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("event store");
    let response_bytes = native_followup_responses();
    let mut request_bytes = Vec::new();
    let mut response_index = 0_usize;
    let run_input = |episode_id| CandidateNativeFollowupEpisodeRunInput {
        search_input: search.clone(),
        recovery_input: recovery.clone(),
        previous_revision: fixture.previous.clone(),
        previous_revision_id: fixture.previous_id,
        build_diagnostic: fixture.diagnostic.clone(),
        episode_id,
        model_configuration,
        selection: ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("deepseek-candidate-native-followup-recorded").expect("model"),
            deployment: DeploymentName::new("local-recorded").expect("deployment"),
            adapter_version: AdapterVersion::new("native-protocol-v1").expect("adapter"),
        },
        budget: EpisodeBudget {
            step_limit: Some(EpisodeStepLimit::new(4).expect("step limit")),
            tool_operation_limit: Some(EpisodeToolOperationLimit::new(8)),
            provider_token_limit: None,
            deadline_unix_ms: None,
            external_meter_limits: None,
        },
        max_output_tokens: ModelOutputTokenLimit::new(16_384).expect("output limit"),
        task_limits: SirTaskLimits::default(),
    };
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
        run_collection_candidate_native_followup_episode(
            &mut events,
            &mut content,
            &mut scripted,
            codec(),
            workspace.clone(),
            run_input(followup_episode),
        )
        .expect("scripted Candidate native follow-up episode")
    };
    assert_eq!(outcome.steps_started(), 2);
    assert_eq!(outcome.search_input(), search.id());
    assert_eq!(outcome.previous_revision_id(), fixture.previous_id);
    assert_eq!(outcome.diagnostic_id(), fixture.diagnostic.id());
    assert_eq!(outcome.followup().episode_id(), followup_episode);
    assert_eq!(
        outcome.followup().model_configuration(),
        model_configuration
    );
    assert_eq!(outcome.followup().search_input(), search.id());
    assert_eq!(outcome.followup().previous_revision(), fixture.previous_id);
    assert_eq!(
        outcome.followup().build_diagnostic(),
        fixture.diagnostic.id()
    );
    assert_ne!(
        outcome.followup().submission(),
        fixture.previous.submission()
    );
    assert_eq!(request_bytes.len(), 2);

    let initial = String::from_utf8(request_bytes[0].clone()).expect("initial request");
    assert!(initial.contains("candidate_submit_native_followup_revision"));
    assert!(initial.contains("void host() { Kernel kernel; }"));
    assert!(initial.contains("call to __aicore__ function from __host__ function"));
    assert!(initial.contains("primary_source_bytes_copied_unchanged_to_fixed_asc_path"));
    assert!(initial.contains("cmake_language"));
    assert!(initial.contains("ASC"));
    assert!(initial.contains("dav-3510"));
    assert!(initial.contains(&fixture.previous_id.to_string()));
    assert!(initial.contains(&fixture.diagnostic.id().to_string()));
    assert!(!initial.contains("candidate-build=complete"));
    for forbidden in [
        "qualification_receipt",
        "comparison_evidence",
        "missing_occurrence",
        "expected_collection",
        "private_continuation",
    ] {
        assert!(!initial.contains(forbidden), "leaked {forbidden}");
    }
    let continuation = String::from_utf8(request_bytes[1].clone()).expect("continuation");
    assert!(continuation.contains("accepted_candidate_native_followup"));
    assert!(continuation.contains(&outcome.followup_id().to_string()));

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
            &AgentEpisode::new(followup_episode).expect("episode")
        )
        .expect("terminal recovery"),
        AgentEpisodeState::Completed {
            reason: EpisodeCompletionReason::Yielded,
            steps_started: 2
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
        .zip(native_followup_responses())
        .map(|(request, response)| RecordedExchange {
            request_id: ContentId::derive(request).expect("request identity"),
            response_bytes: response,
            usage: None,
        })
        .collect::<Vec<_>>();
    let mut recorded = RecordedModelTransport::new(exchanges);
    let replay = run_collection_candidate_native_followup_episode(
        &mut replay_events,
        &mut replay_content,
        &mut recorded,
        codec(),
        workspace,
        run_input(followup_episode),
    )
    .expect("recorded Candidate native follow-up replay");
    assert_eq!(replay.followup_id(), outcome.followup_id());
    assert_eq!(
        replay.previous_revision_id(),
        outcome.previous_revision_id()
    );
    assert_eq!(replay.diagnostic_id(), outcome.diagnostic_id());
}

fn decode_submission(value: &Value) -> Result<CollectionCandidateProposalSubmissionV1, String> {
    cairn_codec::from_slice(&cairn_codec::to_vec(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}
