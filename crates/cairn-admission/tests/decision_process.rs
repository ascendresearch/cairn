use std::{fs, io::Cursor, path::Path, process::Command};

use cairn_migration::{
    IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, SirCapabilityManifestV1, SirResolvedRuntimeModelArtifact,
    SirTaskBundleArtifact, SirTaskLimits,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use serde_json::{Value, json};

fn submission() -> Value {
    json!({
        "schema_version":1,
        "observed_facts":[{
            "id":"atomic-slots","statement":"Output slots are allocated atomically.",
            "citations":[{"path":"src/compact_above.cu","start_line":16,"end_line":20}]
        }],
        "hypotheses":[
            {
                "id":"order-unspecified","layer":"observable-contract",
                "claim":"Any permutation of qualifying values is acceptable.",
                "domain":"Successful calls with sufficient capacity.",
                "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
                "counter_evidence":[]
            },
            {
                "id":"stable-order","layer":"observable-contract",
                "claim":"Qualifying values retain input-relative order.",
                "domain":"Successful calls with sufficient capacity.",
                "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
                "counter_evidence":[{"source":"observed-fact","observation":"atomic-slots"}]
            }
        ],
        "conflicts":[{
            "id":"order-conflict","statement":"The output-order contracts conflict.",
            "claims":[
                {"source":"hypothesis","hypothesis":"order-unspecified"},
                {"source":"hypothesis","hypothesis":"stable-order"}
            ],
            "evidence":[{"source":"observed-fact","observation":"atomic-slots"}]
        }],
        "unknowns":[{
            "id":"output-order","kind":"desired-semantics",
            "question":"Must output preserve input-relative order?",
            "evidence":[{"source":"observed-fact","observation":"atomic-slots"}]
        }],
        "invariants":[{
            "id":"copied-values","statement":"Every output value comes from input.",
            "evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}]
        }],
        "optimization_freedoms":[],
        "source_dispositions":[],
        "disambiguation_experiments":[{
            "id":"decide-order",
            "targets":[
                {"kind":"conflict","conflict":"order-conflict"},
                {"kind":"unknown","unknown":"output-order"}
            ],
            "plan":"Ask the actual task authority whether output ordering is observable.",
            "predictions":["Stable use selects stable-order.","Order-insensitive use selects order-unspecified."]
        }]
    })
}

fn archive_input_and_proposal(
    database: &Path,
    cas: &Path,
) -> (
    ContentId<cairn_migration::SirIntentHypothesisSetProposalArtifact>,
    ContentId<cairn_migration::IntentRecoveryInputArtifact>,
) {
    let caller_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let request: IntentRecoveryRequestV1 =
        serde_json::from_slice(&fs::read(caller_path).expect("caller request"))
            .expect("strict request");
    let input = IntentRecoveryInputV1::new(
        TaskId::new(),
        ContentId::<SirTaskBundleArtifact>::derive(b"process test bundle").expect("bundle"),
        request,
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("input");
    let input_bytes = cairn_codec::to_vec(&input).expect("input bytes");
    let mut store = SqliteContentStore::open(database, cas).expect("content store");
    let input_id = store
        .put::<cairn_migration::IntentRecoveryInputArtifact>(&mut Cursor::new(input_bytes))
        .expect("archive input")
        .content_id;

    let proposal_value = json!({
        "schema_version":1,
        "recovery_input":input_id,
        "episode_id":EpisodeId::new(),
        "model_configuration":ContentId::<SirResolvedRuntimeModelArtifact>::derive(b"recorded model").expect("model"),
        "submission":submission()
    });
    let proposal_bytes = cairn_codec::to_vec(&proposal_value).expect("proposal bytes");
    let proposal: IntentHypothesisSetProposalV1 =
        cairn_codec::from_slice(&proposal_bytes).expect("strict proposal");
    let canonical = cairn_codec::to_vec(&proposal).expect("canonical proposal");
    let proposal_id = store
        .put::<cairn_migration::SirIntentHypothesisSetProposalArtifact>(&mut Cursor::new(canonical))
        .expect("archive proposal")
        .content_id;
    (proposal_id, input_id)
}

#[test]
fn child_process_reads_exact_public_artifacts_and_emits_only_canonical_v1() {
    let state = tempfile::tempdir().expect("state");
    let database = state.path().join("content.db");
    let cas = state.path().join("cas");
    let (proposal_id, input_id) = archive_input_and_proposal(&database, &cas);
    let output = Command::new(env!("CARGO_BIN_EXE_cairn-admission"))
        .args([
            "intent-decision-requests",
            database.to_str().expect("database path"),
            cas.to_str().expect("CAS path"),
            &proposal_id.to_wire(),
        ])
        .env_clear()
        .output()
        .expect("admission process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let batch: IntentDecisionRequestBatchV1 =
        cairn_codec::from_slice(&output.stdout).expect("canonical process output");
    assert_eq!(batch.requests().len(), 1);
    assert_eq!(batch.requests()[0].options().len(), 2);
    assert_eq!(cairn_codec::to_vec(&batch).expect("bytes"), output.stdout);

    let wrong_domain = Command::new(env!("CARGO_BIN_EXE_cairn-admission"))
        .args([
            "intent-decision-requests",
            database.to_str().expect("database path"),
            cas.to_str().expect("CAS path"),
            &input_id.to_wire(),
        ])
        .env_clear()
        .output()
        .expect("wrong-domain process");
    assert!(!wrong_domain.status.success());
    assert!(wrong_domain.stdout.is_empty());
}
