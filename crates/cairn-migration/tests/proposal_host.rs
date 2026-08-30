use std::{fs, path::Path};

use cairn_agent::{
    AdapterVersion, DeploymentName, EpisodeBudget, EpisodeStepLimit, EpisodeToolOperationLimit,
    ModelName, ModelOutputTokenLimit, ModelProtocolConfig, ModelSelection, ModelTransportResponse,
    ProviderName, ResponsesReasoningReplay, ScriptedModelTransport, TransportError,
};
use cairn_execution::{
    DockerImageId, ExecutionEnvironmentArtifact, ExecutionEvidenceArtifact, ExecutionReceipt,
    ExecutionReceiptArtifact, ExecutionStderrArtifact, ExecutionStdoutArtifact,
    InputBundleArtifact, JobContractArtifact,
};
use cairn_migration::{
    AdmittedCollectionOracleClaimArtifact, AgentResolvedRuntimeModelArtifact,
    CandidateBuildEnvironmentProfileV1, CandidateNativeBuildDispatchV1,
    CandidateNativeBuildScheduleV1, CandidateNativeDiagnosticV1, CandidateNativePublicationV1,
    CandidateNativeRepairParentV1, CandidateRevisionRoundLimit, CandidateWorkflowAuthorityV1,
    CandidateWorkflowStateV1, CollectionCandidateNativeBuildDiagnosticArtifact,
    CollectionCandidateNativeBuildDiagnosticV1,
    CollectionCandidateNativeRepairBuildDiagnosticArtifact,
    CollectionCandidateNativeRepairBuildDiagnosticV1, CollectionCandidateNativeRepairRevisionV1,
    CollectionCandidateRevisionV1, CollectionCandidateSearchAuthorityInput,
    CollectionOracleAdmissionPublicOutcomeArtifact, CollectionOracleClaimDomainV1,
    CollectionOracleClaimStrengthV1, IntentRecoveryInputV1, IntentRecoveryRequestV1,
    MigrationIntentContractArtifact, ProposalHostBinaryIdentity, ProposalHostPublicationV1,
    ProposalHostRequestV1, ProposalHostRoleRequestV1, ProposalHostRuntimeV1,
    ProposalHostTaskSnapshotV1, SirCallerClaimId, SirCapabilityManifestV1, SirTaskLimits,
    SirTaskWorkspace, open_candidate_workflow, prepare_collection_candidate_search_input,
    record_candidate_native_subject_failure, record_candidate_proposal_host_terminal,
    request_candidate_episode, request_candidate_native_build, run_proposal_host_episode,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentId, ContentType, ControlMessageId, EpisodeId, JobId,
    LeaseId, ObservedAtUnixMillis, PlacementId, ReservationId, TaskId,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};
use serde_json::{Value, json};

fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
    ContentId::derive(label).expect("content identity")
}

fn codec() -> cairn_agent::NativeProtocolCodec {
    cairn_agent::NativeProtocolCodec::from_config(&ModelProtocolConfig::OpenAiResponses {
        store: false,
        reasoning_replay: ResponsesReasoningReplay::PreserveOutputItems,
    })
    .expect("native codec")
}

fn runtime(episode_id: EpisodeId, label: &[u8]) -> ProposalHostRuntimeV1 {
    ProposalHostRuntimeV1::new(
        episode_id,
        ProposalHostBinaryIdentity::new(format!("sha256:{}", "1".repeat(64)))
            .expect("Host binary identity"),
        id::<AgentResolvedRuntimeModelArtifact>(label),
        ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("recorded-role-model").expect("model"),
            deployment: DeploymentName::new("isolated-recorded").expect("deployment"),
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
    )
}

fn request(value: &Value) -> IntentRecoveryRequestV1 {
    cairn_codec::from_slice(&cairn_codec::to_vec(&value).expect("request bytes"))
        .expect("strict request")
}

fn sir_request() -> IntentRecoveryRequestV1 {
    request(&json!({
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
    }))
}

fn candidate_request() -> IntentRecoveryRequestV1 {
    request(&json!({
        "schema_version":1,
        "caller":{
            "schema_version":1,
            "source_entry_point":"launch_select",
            "arguments":[
                {"index":0,"name":"input","role":"input-buffer","data_type":"f32","shape":{"kind":"ranked","dimensions":["count"]},"valid_domain":"Readable finite normal binary32 elements."},
                {"index":1,"name":"count","role":"scalar","data_type":"u32","shape":{"kind":"scalar"},"valid_domain":"Logical input length and output capacity."},
                {"index":2,"name":"threshold","role":"scalar","data_type":"f32","shape":{"kind":"scalar"},"valid_domain":"Finite normal binary32 threshold."},
                {"index":3,"name":"output","role":"output-buffer","data_type":"f32","shape":{"kind":"ranked","dimensions":["count"]},"valid_domain":"Writable output."},
                {"index":4,"name":"output_count","role":"output-buffer","data_type":"u32","shape":{"kind":"ranked","dimensions":["1"]},"valid_domain":"Writable count."}
            ],
            "error_behaviors":["Return the caller-visible launch status."],
            "claims":[
                {"id":"copies-strictly-above","layer":"algorithm","statement":"Copy every occurrence strictly above threshold.","references":[]},
                {"id":"reported-count","layer":"observable-contract","statement":"Report the exact selected occurrence count.","references":[]}
            ],
            "exclusions":[],
            "unknowns":[{"id":"output-order","kind":"observable-contract","question":"Is output order observable?"}]
        },
        "target":{"soc":{"kind":"not-selected"},"toolchain":{"kind":"not-selected"},"environment":{"kind":"not-selected"}},
        "authorized_evidence":[],
        "prior_feedback":{"kind":"no-prior-feedback"}
    }))
}

fn write_sir_task(root: &Path) {
    fs::write(
        root.join("transform.cu"),
        "__global__ void transform(const float* input, float* output) {\n  int i = blockIdx.x * blockDim.x + threadIdx.x;\n  output[i] = input[i];\n}\n",
    )
    .expect("SIR task");
}

fn write_candidate_task(root: &Path) {
    fs::write(
        root.join("select.cu"),
        "__global__ void select(const float* input, unsigned count, float threshold, float* output, unsigned* output_count) {\n  unsigned i = blockIdx.x * blockDim.x + threadIdx.x;\n  if (i < count && input[i] > threshold) output[atomicAdd(output_count, 1U)] = input[i];\n}\n",
    )
    .expect("Candidate task");
}

fn sir_responses() -> Vec<Vec<u8>> {
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
    let invalid_submit = serde_json::to_string(&json!({
        "schema_version":1,
        "observed_facts":[],
        "hypotheses":[],
        "conflicts":[],
        "unknowns":[],
        "invariants":[],
        "optimization_freedoms":[],
        "source_dispositions":[],
        "disambiguation_experiments":[]
    }))
    .expect("invalid submit args");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"sir-read","name":"sir_read_task_artifact","arguments":read}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"sir-invalid-submit","name":"sir_submit_intent_hypotheses","arguments":invalid_submit}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"sir-submit","name":"sir_submit_intent_hypotheses","arguments":submit}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"sir-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"submitted"}]}]})).expect("response"),
    ]
}

fn candidate_responses() -> Vec<Vec<u8>> {
    let submit = serde_json::to_string(&json!({
        "schema_version":1,
        "files":[{"path":"src/select.asc","source":"#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void select_kernel() {}\n"}],
        "primary_source":"src/select.asc",
        "explanation":"A complete non-authoritative source proposal for the frozen public search input."
    }))
    .expect("Candidate submit args");
    vec![
        serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"candidate-submit","name":"candidate_submit_collection_proposal","arguments":submit}]})).expect("response"),
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"candidate-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"submitted"}]}]})).expect("response"),
    ]
}

fn run_with_responses(
    request: ProposalHostRequestV1,
    responses: Vec<Vec<u8>>,
) -> cairn_migration::ProposalHostTerminalV1 {
    let state = tempfile::tempdir().expect("state");
    let mut content =
        SqliteContentStore::open(state.path().join("content.db"), state.path().join("cas"))
            .expect("content");
    let mut events = SqliteEventStore::open(state.path().join("events.db")).expect("events");
    let mut index = 0_usize;
    let mut transport = ScriptedModelTransport::new(
        move |_: &cairn_agent::PreparedModelRequest| -> Result<_, TransportError> {
            let response = responses.get(index).expect("recorded response").clone();
            index += 1;
            Ok(ModelTransportResponse::without_usage(response))
        },
    );
    match run_proposal_host_episode(&mut events, &mut content, &mut transport, codec(), request)
        .expect("Host episode")
    {
        cairn_migration::ProposalHostOutcomeV1::Terminal { terminal } => *terminal,
        cairn_migration::ProposalHostOutcomeV1::AwaitingController { .. } => {
            panic!("recorded local-tool profile unexpectedly requested a Controller experiment")
        }
    }
}

#[test]
fn one_host_lifecycle_atomically_rejects_repairs_and_isolates_sir_and_candidate_profiles() {
    let sir_task = tempfile::tempdir().expect("SIR task");
    write_sir_task(sir_task.path());
    let sir_workspace =
        SirTaskWorkspace::load(sir_task.path(), SirTaskLimits::default()).expect("SIR workspace");
    let sir_episode = EpisodeId::new();
    let sir = ProposalHostRequestV1::new(
        runtime(sir_episode, b"SIR runtime"),
        ProposalHostRoleRequestV1::Sir {
            task_id: TaskId::new(),
            recovery_request: sir_request(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&sir_workspace),
        },
    )
    .expect("SIR Host request");
    let sir_bytes = cairn_codec::to_vec(&sir).expect("SIR request bytes");
    let sir: ProposalHostRequestV1 =
        cairn_codec::from_slice(&sir_bytes).expect("validated SIR request decode");
    let sir_terminal = run_with_responses(sir.clone(), sir_responses());
    sir_terminal.validate_against(&sir).expect("SIR terminal");
    assert!(matches!(
        sir_terminal.publication(),
        ProposalHostPublicationV1::Sir { .. }
    ));

    let candidate_task = tempfile::tempdir().expect("Candidate task");
    write_candidate_task(candidate_task.path());
    let candidate_workspace =
        SirTaskWorkspace::load(candidate_task.path(), SirTaskLimits::default())
            .expect("Candidate workspace");
    let task_id = TaskId::new();
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        candidate_workspace.bundle().identity().expect("bundle id"),
        candidate_request(),
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery");
    let search =
        prepare_collection_candidate_search_input(&CollectionCandidateSearchAuthorityInput::new(
            task_id,
            recovery.identity().expect("recovery id"),
            id::<MigrationIntentContractArtifact>(b"intent"),
            id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"oracle outcome"),
            id::<AdmittedCollectionOracleClaimArtifact>(b"oracle claim"),
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
            CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        ))
        .expect("search");
    let candidate_episode = EpisodeId::new();
    let candidate = ProposalHostRequestV1::new(
        runtime(candidate_episode, b"Candidate runtime"),
        ProposalHostRoleRequestV1::CandidateInitial {
            recovery_input: recovery,
            search_input: search.input().clone(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&candidate_workspace),
        },
    )
    .expect("Candidate Host request");
    let candidate_terminal = run_with_responses(candidate.clone(), candidate_responses());
    candidate_terminal
        .validate_against(&candidate)
        .expect("Candidate terminal");
    assert!(matches!(
        candidate_terminal.publication(),
        ProposalHostPublicationV1::CandidateInitial { .. }
    ));
    assert_ne!(sir_terminal.episode_id(), candidate_terminal.episode_id());
    assert_ne!(sir_terminal.request(), candidate_terminal.request());
}

#[test]
fn host_request_rejects_task_snapshot_and_non_v1_drift() {
    let task = tempfile::tempdir().expect("task");
    write_sir_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let request = ProposalHostRequestV1::new(
        runtime(EpisodeId::new(), b"validation runtime"),
        ProposalHostRoleRequestV1::Sir {
            task_id: TaskId::new(),
            recovery_request: sir_request(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
        },
    )
    .expect("request");
    let mut value = serde_json::to_value(&request).expect("value");
    value["schema_version"] = json!(2);
    assert!(
        cairn_codec::from_slice::<ProposalHostRequestV1>(
            &cairn_codec::to_vec(&value).expect("bytes")
        )
        .is_err()
    );
    let mut value = serde_json::to_value(&request).expect("value");
    value["role"]["task"]["sources"][0]["source"] = json!("changed source\n");
    assert!(
        cairn_codec::from_slice::<ProposalHostRequestV1>(
            &cairn_codec::to_vec(&value).expect("bytes")
        )
        .is_err()
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn workflow_candidate_request_is_consumed_and_publication_returns_to_same_aggregate() {
    let task = tempfile::tempdir().expect("Candidate task");
    write_candidate_task(task.path());
    let workspace =
        SirTaskWorkspace::load(task.path(), SirTaskLimits::default()).expect("workspace");
    let task_id = TaskId::new();
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        workspace.bundle().identity().expect("bundle id"),
        candidate_request(),
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery");
    let search =
        prepare_collection_candidate_search_input(&CollectionCandidateSearchAuthorityInput::new(
            task_id,
            recovery.identity().expect("recovery id"),
            id::<MigrationIntentContractArtifact>(b"workflow intent"),
            id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"workflow outcome"),
            id::<AdmittedCollectionOracleClaimArtifact>(b"workflow claim"),
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
            CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        ))
        .expect("search");
    let revision_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "search_input":search.id(),
        "parent_proposal":id::<cairn_migration::CollectionCandidateProposalArtifact>(b"workflow parent"),
        "build_diagnostic":id::<cairn_migration::CollectionCandidateBuildDiagnosticArtifact>(b"workflow initial diagnostic"),
        "episode_id":EpisodeId::new(),
        "model_configuration":id::<AgentResolvedRuntimeModelArtifact>(b"workflow previous model"),
        "submission":{
            "schema_version":1,
            "files":[{"path":"src/previous.asc","source":"#include \"kernel_operator.h\"\nvoid previous() {}\n"}],
            "primary_source":"src/previous.asc",
            "explanation":"Complete previous source before exact native compiler feedback."
        }
    }))
    .expect("revision bytes");
    let revision: CollectionCandidateRevisionV1 =
        cairn_codec::from_slice(&revision_bytes).expect("revision");
    let revision_id = revision.identity().expect("revision id");
    let workflow = cairn_migration::MigrationWorkflowV1::new(task_id).expect("workflow");
    let mut events =
        SqliteEventStore::open(task.path().join("workflow-events.db")).expect("workflow events");
    open_candidate_workflow(
        &mut events,
        &workflow,
        CandidateWorkflowAuthorityV1::from_search_input(search.id(), search.input())
            .expect("authority"),
        search.input(),
        &revision,
        revision_id,
        DockerImageId::new(format!("sha256:{}", "a".repeat(64))).expect("image"),
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
        CandidateRevisionRoundLimit::new(1).expect("revision limit"),
        &CommandId::new(),
        ObservedAtUnixMillis::new(1),
    )
    .expect("open workflow");
    let schedule = CandidateNativeBuildScheduleV1 {
        attempt_id: AttemptId::new(),
        placement_id: PlacementId::new(),
        reservation_id: ReservationId::new(),
        assignment_id: AssignmentId::new(),
        lease_id: LeaseId::new(),
        offer_message_id: ControlMessageId::new(),
        start_message_id: ControlMessageId::new(),
        authorize_attempt_command: CommandId::new(),
        reserve_placement_command: CommandId::new(),
        grant_assignment_command: CommandId::new(),
        enqueue_offer_command: CommandId::new(),
    };
    let dispatch = CandidateNativeBuildDispatchV1::new(
        CandidateNativePublicationV1::Revision(revision_id),
        JobId::new(),
        id::<InputBundleArtifact>(b"workflow input bundle"),
        id::<ExecutionEnvironmentArtifact>(b"workflow environment"),
        id::<JobContractArtifact>(b"workflow contract"),
        schedule,
    );
    request_candidate_native_build(
        &mut events,
        &workflow,
        dispatch.clone(),
        &CommandId::new(),
        ObservedAtUnixMillis::new(2),
    )
    .expect("build request");
    let receipt_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "job_id":dispatch.job_id(),
        "attempt_id":schedule.attempt_id,
        "contract_id":dispatch.contract(),
        "outcome":"subject-failed",
        "exit_code":1,
        "elapsed_ms":10,
        "stdout_id":id::<ExecutionStdoutArtifact>(b"workflow stdout"),
        "stderr_id":id::<ExecutionStderrArtifact>(b"workflow stderr"),
        "evidence_id":id::<ExecutionEvidenceArtifact>(b"workflow evidence"),
        "outputs":[]
    }))
    .expect("receipt bytes");
    let receipt: ExecutionReceipt = cairn_codec::from_slice(&receipt_bytes).expect("receipt");
    let receipt_id =
        ContentId::<ExecutionReceiptArtifact>::derive(&receipt_bytes).expect("receipt id");
    let diagnostic_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "previous_revision":revision_id,
        "input_bundle":dispatch.input_bundle(),
        "environment":dispatch.environment(),
        "contract":dispatch.contract(),
        "receipt":receipt_id,
        "stderr":receipt.stderr_id(),
        "evidence":receipt.evidence_id(),
        "diagnostic":"candidate_primary.asc: error: exact recorded native diagnostic\n"
    }))
    .expect("diagnostic bytes");
    let diagnostic: CollectionCandidateNativeBuildDiagnosticV1 =
        cairn_codec::from_slice(&diagnostic_bytes).expect("diagnostic");
    let diagnostic_id =
        ContentId::<CollectionCandidateNativeBuildDiagnosticArtifact>::derive(&diagnostic_bytes)
            .expect("diagnostic id");
    record_candidate_native_subject_failure(
        &mut events,
        &workflow,
        receipt_id,
        &receipt,
        CandidateNativeDiagnosticV1::NativeFollowup(diagnostic_id),
        &CommandId::new(),
        ObservedAtUnixMillis::new(3),
    )
    .expect("subject failure");
    let episode_id = EpisodeId::new();
    let host_runtime = runtime(episode_id, b"workflow Host runtime");
    let requested = request_candidate_episode(
        &mut events,
        &workflow,
        episode_id,
        host_runtime.identity().expect("invocation id"),
        &CommandId::new(),
        ObservedAtUnixMillis::new(4),
    )
    .expect("episode request");
    let CandidateWorkflowStateV1::CandidateEpisodeRequested {
        request: workflow_request,
        ..
    } = requested
    else {
        panic!("workflow did not persist episode request");
    };
    let host_request = ProposalHostRequestV1::new(
        host_runtime,
        ProposalHostRoleRequestV1::CandidateNativeFollowup {
            workflow_request,
            recovery_input: recovery,
            search_input: search.input().clone(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
            previous_revision: revision,
            diagnostic,
        },
    )
    .expect("Host request");
    let terminal = run_with_responses(host_request.clone(), vec![
        {
            let arguments = serde_json::to_string(&json!({
                "schema_version":1,
                "files":[{"path":"src/followup.asc","source":"#include \"kernel_operator.h\"\nextern \"C\" __global__ __aicore__ void followup() {}\n"}],
                "primary_source":"src/followup.asc",
                "explanation":"Changed complete source after the exact native diagnostic."
            })).expect("follow-up args");
            serde_json::to_vec(&json!({"output":[{"type":"function_call","call_id":"followup-submit","name":"candidate_submit_native_followup_revision","arguments":arguments}]})).expect("response")
        },
        serde_json::to_vec(&json!({"output":[{"type":"message","id":"followup-final","phase":"final_answer","role":"assistant","status":"completed","content":[{"type":"output_text","text":"submitted"}]}]})).expect("response"),
    ]);
    let state = record_candidate_proposal_host_terminal(
        &mut events,
        &workflow,
        &host_request,
        &terminal,
        &CommandId::new(),
        ObservedAtUnixMillis::new(5),
    )
    .expect("record Host publication");
    assert!(matches!(
        state,
        CandidateWorkflowStateV1::ReadyForNativeBuild {
            publication: CandidateNativePublicationV1::NativeFollowup(_),
            ..
        }
    ));

    let ProposalHostPublicationV1::CandidateNativeFollowup {
        followup_id,
        followup,
    } = terminal.publication()
    else {
        panic!("Host did not return the root native follow-up");
    };
    let ProposalHostRoleRequestV1::CandidateNativeFollowup {
        workflow_request: followup_request,
        recovery_input: host_recovery,
        ..
    } = host_request.role()
    else {
        panic!("Host request changed role");
    };
    let first_repair_diagnostic_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "parent":CandidateNativeRepairParentV1::RootFollowup(*followup_id),
        "input_bundle":id::<InputBundleArtifact>(b"repair input bundle"),
        "environment":id::<ExecutionEnvironmentArtifact>(b"repair environment"),
        "contract":id::<JobContractArtifact>(b"repair contract"),
        "receipt":id::<ExecutionReceiptArtifact>(b"repair receipt"),
        "stderr":id::<ExecutionStderrArtifact>(b"repair stderr"),
        "evidence":id::<ExecutionEvidenceArtifact>(b"repair evidence"),
        "diagnostic":"candidate_primary.asc: error: exact first repair diagnostic\n"
    }))
    .expect("repair diagnostic bytes");
    let first_repair_diagnostic: CollectionCandidateNativeRepairBuildDiagnosticV1 =
        cairn_codec::from_slice(&first_repair_diagnostic_bytes).expect("repair diagnostic");
    let first_repair_diagnostic_id = ContentId::<
        CollectionCandidateNativeRepairBuildDiagnosticArtifact,
    >::derive(&first_repair_diagnostic_bytes)
    .expect("repair diagnostic id");
    let first_repair_runtime = runtime(EpisodeId::new(), b"first repair runtime");
    let first_repair_request: cairn_migration::CandidateEpisodeRequestV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "kind":"native-repair",
            "episode_id":first_repair_runtime.episode_id(),
            "authority":followup_request.authority(),
            "parent":CandidateNativePublicationV1::NativeFollowup(*followup_id),
            "diagnostic":CandidateNativeDiagnosticV1::NativeRepair(first_repair_diagnostic_id),
            "revision_round":1,
            "invocation":first_repair_runtime.identity().expect("invocation")
        }))
        .expect("repair request bytes"),
    )
    .expect("repair request");
    ProposalHostRequestV1::new(
        first_repair_runtime,
        ProposalHostRoleRequestV1::CandidateNativeRepair {
            workflow_request: first_repair_request,
            recovery_input: host_recovery.clone(),
            search_input: search.input().clone(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
            root_followup: followup.clone(),
            parent_repair: None,
            diagnostic: first_repair_diagnostic,
        },
    )
    .expect("first repair Host lineage");

    let parent_repair_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "search_input":search.id(),
        "root_followup":followup_id,
        "parent":CandidateNativeRepairParentV1::RootFollowup(*followup_id),
        "build_diagnostic":first_repair_diagnostic_id,
        "episode_id":EpisodeId::new(),
        "model_configuration":id::<AgentResolvedRuntimeModelArtifact>(b"parent repair model"),
        "submission":{
            "schema_version":1,
            "files":[{"path":"src/repair.asc","source":"#include \"kernel_operator.h\"\nvoid repair() {}\n"}],
            "primary_source":"src/repair.asc",
            "explanation":"Complete parent repair source for lineage validation."
        }
    }))
    .expect("parent repair bytes");
    let parent_repair: CollectionCandidateNativeRepairRevisionV1 =
        cairn_codec::from_slice(&parent_repair_bytes).expect("parent repair");
    let parent_repair_id = parent_repair.identity().expect("parent repair id");
    let later_diagnostic_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "parent":CandidateNativeRepairParentV1::Repair(parent_repair_id),
        "input_bundle":id::<InputBundleArtifact>(b"later repair input bundle"),
        "environment":id::<ExecutionEnvironmentArtifact>(b"later repair environment"),
        "contract":id::<JobContractArtifact>(b"later repair contract"),
        "receipt":id::<ExecutionReceiptArtifact>(b"later repair receipt"),
        "stderr":id::<ExecutionStderrArtifact>(b"later repair stderr"),
        "evidence":id::<ExecutionEvidenceArtifact>(b"later repair evidence"),
        "diagnostic":"candidate_primary.asc: error: exact later repair diagnostic\n"
    }))
    .expect("later diagnostic bytes");
    let later_diagnostic: CollectionCandidateNativeRepairBuildDiagnosticV1 =
        cairn_codec::from_slice(&later_diagnostic_bytes).expect("later diagnostic");
    let later_diagnostic_id =
        ContentId::<CollectionCandidateNativeRepairBuildDiagnosticArtifact>::derive(
            &later_diagnostic_bytes,
        )
        .expect("later diagnostic id");
    let later_runtime = runtime(EpisodeId::new(), b"later repair runtime");
    let later_request: cairn_migration::CandidateEpisodeRequestV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "kind":"native-repair",
            "episode_id":later_runtime.episode_id(),
            "authority":followup_request.authority(),
            "parent":CandidateNativePublicationV1::NativeRepair(parent_repair_id),
            "diagnostic":CandidateNativeDiagnosticV1::NativeRepair(later_diagnostic_id),
            "revision_round":2,
            "invocation":later_runtime.identity().expect("invocation")
        }))
        .expect("later request bytes"),
    )
    .expect("later request");
    assert!(
        ProposalHostRequestV1::new(
            later_runtime.clone(),
            ProposalHostRoleRequestV1::CandidateNativeRepair {
                workflow_request: later_request.clone(),
                recovery_input: host_recovery.clone(),
                search_input: search.input().clone(),
                task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
                root_followup: followup.clone(),
                parent_repair: None,
                diagnostic: later_diagnostic.clone(),
            },
        )
        .is_err()
    );
    ProposalHostRequestV1::new(
        later_runtime,
        ProposalHostRoleRequestV1::CandidateNativeRepair {
            workflow_request: later_request,
            recovery_input: host_recovery.clone(),
            search_input: search.input().clone(),
            task: ProposalHostTaskSnapshotV1::from_workspace(&workspace),
            root_followup: followup.clone(),
            parent_repair: Some(Box::new(parent_repair)),
            diagnostic: later_diagnostic,
        },
    )
    .expect("later repair Host lineage");
}
