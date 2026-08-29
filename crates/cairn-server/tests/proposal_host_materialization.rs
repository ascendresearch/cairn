use std::{fs, io::Cursor};

use cairn_agent::{
    AdapterVersion, DeploymentName, EpisodeBudget, EpisodeStepLimit, EpisodeToolOperationLimit,
    ModelName, ModelOutputTokenLimit, ModelSelection, ProviderName,
};
use cairn_migration::{
    AdmittedCollectionOracleClaimArtifact, AgentResolvedRuntimeModelArtifact,
    CandidateEpisodeRequestV1, CandidateNativeDiagnosticV1, CandidateNativePublicationV1,
    CandidateWorkflowAuthorityV1, CollectionCandidateNativeBuildDiagnosticArtifact,
    CollectionCandidateNativeBuildDiagnosticV1, CollectionCandidateProposalArtifact,
    CollectionCandidateRevisionArtifact, CollectionCandidateRevisionV1,
    CollectionCandidateSearchAuthorityInput, CollectionCandidateSearchInputArtifact,
    CollectionOracleAdmissionPublicOutcomeArtifact, CollectionOracleClaimDomainV1,
    CollectionOracleClaimStrengthV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, MigrationIntentContractArtifact, ProposalHostRoleRequestV1,
    ProposalHostRuntimeV1, SirCallerClaimId, SirCapabilityManifestV1, SirTaskArtifactBytes,
    SirTaskBundleArtifact, SirTaskLimits, SirTaskWorkspace,
    prepare_collection_candidate_search_input,
};
use cairn_protocol::{ContentId, ContentType, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_server::{
    ServerConfig, archive_proposal_host_runtime, prepare_candidate_proposal_host_request,
};
use cairn_store_sqlite::SqliteContentStore;
use serde::Serialize;
use serde_json::{Value, json};

fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
    ContentId::derive(label).expect("content identity")
}

fn archive<T: ContentType, V: Serialize>(
    store: &mut SqliteContentStore,
    value: &V,
) -> ContentId<T> {
    store
        .put::<T>(&mut Cursor::new(
            cairn_codec::to_vec(value).expect("canonical bytes"),
        ))
        .expect("archive")
        .content_id
}

fn candidate_request() -> IntentRecoveryRequestV1 {
    let value = json!({
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
    });
    cairn_codec::from_slice(&cairn_codec::to_vec(&value).expect("request bytes")).expect("request")
}

fn config(root: &std::path::Path) -> ServerConfig {
    let mut value: Value =
        serde_json::from_str(include_str!("../../../config/controller.example.json"))
            .expect("example config");
    value["storage"]["event_database"] =
        json!(root.join("events.db").to_string_lossy().into_owned());
    value["storage"]["content_database"] =
        json!(root.join("content.db").to_string_lossy().into_owned());
    value["storage"]["content_directory"] = json!(root.join("cas").to_string_lossy().into_owned());
    serde_json::from_value(value).expect("server config")
}

#[test]
#[allow(clippy::too_many_lines)]
fn controller_rebuilds_exact_followup_host_request_from_public_cas() {
    let temporary = tempfile::tempdir().expect("temporary root");
    let task_root = temporary.path().join("task");
    fs::create_dir(&task_root).expect("task directory");
    fs::write(
        task_root.join("select.cu"),
        "__global__ void select(const float* input, unsigned count, float threshold, float* output, unsigned* output_count) {\n  unsigned i = blockIdx.x * blockDim.x + threadIdx.x;\n  if (i < count && input[i] > threshold) output[atomicAdd(output_count, 1U)] = input[i];\n}\n",
    )
    .expect("task source");
    let workspace =
        SirTaskWorkspace::load(&task_root, SirTaskLimits::default()).expect("workspace");
    let task_id = TaskId::new();
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        workspace.bundle().identity().expect("bundle identity"),
        candidate_request(),
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery");
    let search =
        prepare_collection_candidate_search_input(&CollectionCandidateSearchAuthorityInput::new(
            task_id,
            recovery.identity().expect("recovery identity"),
            id::<MigrationIntentContractArtifact>(b"intent"),
            id::<CollectionOracleAdmissionPublicOutcomeArtifact>(b"outcome"),
            id::<AdmittedCollectionOracleClaimArtifact>(b"claim"),
            SirCallerClaimId::new("copies-strictly-above").expect("claim id"),
            CollectionOracleClaimDomainV1::FiniteNormalF32StrictlyAboveThreshold,
            CollectionOracleClaimStrengthV1::ExactOccurrenceMultisetAndReportedCount,
        ))
        .expect("search input");
    let revision: CollectionCandidateRevisionV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "schema_version":1,
            "search_input":search.id(),
            "parent_proposal":id::<CollectionCandidateProposalArtifact>(b"parent"),
            "build_diagnostic":id::<cairn_migration::CollectionCandidateBuildDiagnosticArtifact>(b"initial diagnostic"),
            "episode_id":EpisodeId::new(),
            "model_configuration":id::<AgentResolvedRuntimeModelArtifact>(b"previous model"),
            "submission":{
                "schema_version":1,
                "files":[{"path":"src/previous.asc","source":"#include \"kernel_operator.h\"\nvoid previous() {}\n"}],
                "primary_source":"src/previous.asc",
                "explanation":"Complete previous source before exact native compiler feedback."
            }
        }))
        .expect("revision bytes"),
    )
    .expect("revision");
    let revision_id = revision.identity().expect("revision identity");
    let diagnostic_bytes = cairn_codec::to_vec(&json!({
        "schema_version":1,
        "previous_revision":revision_id,
        "input_bundle":id::<cairn_execution::InputBundleArtifact>(b"bundle"),
        "environment":id::<cairn_execution::ExecutionEnvironmentArtifact>(b"environment"),
        "contract":id::<cairn_execution::JobContractArtifact>(b"contract"),
        "receipt":id::<cairn_execution::ExecutionReceiptArtifact>(b"receipt"),
        "stderr":id::<cairn_execution::ExecutionStderrArtifact>(b"stderr"),
        "evidence":id::<cairn_execution::ExecutionEvidenceArtifact>(b"evidence"),
        "diagnostic":"candidate_primary.asc: error: exact recorded native diagnostic\n"
    }))
    .expect("diagnostic bytes");
    let diagnostic: CollectionCandidateNativeBuildDiagnosticV1 =
        cairn_codec::from_slice(&diagnostic_bytes).expect("diagnostic");
    let diagnostic_id = ContentId::derive(&diagnostic_bytes).expect("diagnostic identity");
    let episode_id = EpisodeId::new();
    let runtime = ProposalHostRuntimeV1::new(
        episode_id,
        id::<AgentResolvedRuntimeModelArtifact>(b"runtime model"),
        ModelSelection {
            provider: ProviderName::new("recorded").expect("provider"),
            model: ModelName::new("recorded-model").expect("model"),
            deployment: DeploymentName::new("isolated").expect("deployment"),
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
    );
    let server = config(temporary.path());
    let invocation = archive_proposal_host_runtime(&server, &runtime).expect("runtime archive");
    let authority = CandidateWorkflowAuthorityV1::from_search_input(search.id(), search.input())
        .expect("authority");
    let workflow_request: CandidateEpisodeRequestV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "kind":"native-followup",
            "episode_id":episode_id,
            "authority":authority,
            "parent":CandidateNativePublicationV1::Revision(revision_id),
            "diagnostic":CandidateNativeDiagnosticV1::NativeFollowup(diagnostic_id),
            "revision_round":1,
            "invocation":invocation
        }))
        .expect("workflow request bytes"),
    )
    .expect("workflow request");

    let mut content = SqliteContentStore::open(
        &server.storage.content_database,
        &server.storage.content_directory,
    )
    .expect("content store");
    assert_eq!(
        archive::<SirTaskBundleArtifact, _>(&mut content, workspace.bundle()),
        recovery.task_bundle()
    );
    for artifact in workspace.bundle().artifacts() {
        let source = fs::read(task_root.join(artifact.path().as_str())).expect("source");
        assert_eq!(
            content
                .put::<SirTaskArtifactBytes>(&mut Cursor::new(source))
                .expect("source archive")
                .content_id,
            artifact.identity()
        );
    }
    assert_eq!(
        archive::<IntentRecoveryInputArtifact, _>(&mut content, &recovery),
        recovery.identity().expect("recovery identity")
    );
    assert_eq!(
        archive::<CollectionCandidateSearchInputArtifact, _>(&mut content, search.input()),
        search.id()
    );
    assert_eq!(
        archive::<CollectionCandidateRevisionArtifact, _>(&mut content, &revision),
        revision_id
    );
    assert_eq!(
        archive::<CollectionCandidateNativeBuildDiagnosticArtifact, _>(&mut content, &diagnostic),
        diagnostic_id
    );
    drop(content);

    let host = prepare_candidate_proposal_host_request(&server, workflow_request.clone())
        .expect("Host request");
    assert_eq!(host.runtime(), &runtime);
    let ProposalHostRoleRequestV1::CandidateNativeFollowup {
        workflow_request: materialized,
        recovery_input,
        search_input,
        previous_revision,
        diagnostic: materialized_diagnostic,
        ..
    } = host.role()
    else {
        panic!("Controller changed the requested Host role");
    };
    assert_eq!(materialized, &workflow_request);
    assert_eq!(recovery_input, &recovery);
    assert_eq!(search_input, search.input());
    assert_eq!(previous_revision, &revision);
    assert_eq!(materialized_diagnostic, &diagnostic);
}
