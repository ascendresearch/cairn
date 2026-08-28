use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputV1, IntentRecoveryRequestV1,
    SirCapabilityManifestV1, SirProcessRequestV1, SirProcessTerminalV1, SirTaskBundleV1,
    SirTaskLimits,
};
use cairn_protocol::{ContentId, EpisodeId, OperationId, SirRunId, TaskId};
use serde_json::json;

fn process_request() -> SirProcessRequestV1 {
    let caller_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let caller: IntentRecoveryRequestV1 =
        serde_json::from_slice(&fs::read(caller_path).expect("caller request"))
            .expect("strict caller request");
    let source = b"atomic output slot allocation\nwrite selected value\n";
    let task_bundle: SirTaskBundleV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "schema_version":1,
            "artifacts":[{
                "path":"src/compact_above.cu",
                "identity":ContentId::<cairn_migration::SirTaskArtifactBytes>::derive(source).expect("source identity"),
                "line_count":2
            }]
        })).expect("bundle bytes")
    ).expect("task bundle");
    let recovery_input = IntentRecoveryInputV1::new(
        TaskId::new(),
        task_bundle.identity().expect("bundle identity"),
        caller,
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery input");
    let proposal: IntentHypothesisSetProposalV1 = cairn_codec::from_slice(
        &cairn_codec::to_vec(&json!({
            "schema_version":1,
            "recovery_input":recovery_input.identity().expect("input identity"),
            "episode_id":EpisodeId::new(),
            "model_configuration":ContentId::<cairn_migration::SirResolvedRuntimeModelArtifact>::derive(b"recorded model").expect("model"),
            "submission":{
                "schema_version":1,
                "observed_facts":[{"id":"atomic-slots","statement":"Output slots are allocated atomically.","citations":[{"path":"src/compact_above.cu","start_line":1,"end_line":2}]}],
                "hypotheses":[
                    {"id":"order-unspecified","layer":"observable-contract","claim":"Any permutation of qualifying values is acceptable.","domain":"Successful calls.","supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],"counter_evidence":[]},
                    {"id":"stable-order","layer":"observable-contract","claim":"Qualifying values retain input-relative order.","domain":"Successful calls.","supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],"counter_evidence":[{"source":"observed-fact","observation":"atomic-slots"}]}
                ],
                "conflicts":[{"id":"order-conflict","statement":"The output-order contracts conflict.","claims":[{"source":"hypothesis","hypothesis":"order-unspecified"},{"source":"hypothesis","hypothesis":"stable-order"}],"evidence":[{"source":"observed-fact","observation":"atomic-slots"}]}],
                "unknowns":[{"id":"output-order","kind":"desired-semantics","question":"Must output preserve input-relative order?","evidence":[{"source":"observed-fact","observation":"atomic-slots"}]}],
                "invariants":[{"id":"copied-values","statement":"Every output value comes from input.","evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}]}],
                "optimization_freedoms":[],
                "source_dispositions":[],
                "disambiguation_experiments":[{"id":"decide-order","targets":[{"kind":"conflict","conflict":"order-conflict"},{"kind":"unknown","unknown":"output-order"}],"plan":"Ask the actual task authority whether output ordering is observable.","predictions":["Stable use selects stable-order.","Order-insensitive use selects order-unspecified."]}]
            }
        })).expect("proposal bytes")
    ).expect("proposal");
    SirProcessRequestV1::new(
        SirRunId::new(),
        OperationId::new(),
        task_bundle,
        recovery_input,
        proposal,
    )
    .expect("process request")
}

#[test]
fn isolated_process_accepts_only_canonical_materialized_v1() {
    let request = process_request();
    let request_bytes = cairn_codec::to_vec(&request).expect("request bytes");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-sir"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("SIR process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&request_bytes)
        .expect("write request");
    let output = child.wait_with_output().expect("process output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let terminal: SirProcessTerminalV1 =
        cairn_codec::from_slice(&output.stdout).expect("strict terminal");
    assert_eq!(
        terminal.proposal_id(),
        terminal.proposal().identity().expect("proposal identity")
    );
    assert_eq!(
        cairn_codec::to_vec(&terminal).expect("canonical terminal"),
        output.stdout
    );

    let mut noncanonical = request_bytes;
    noncanonical.push(b'\n');
    let mut rejected = Command::new(env!("CARGO_BIN_EXE_cairn-sir"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rejecting process");
    rejected
        .stdin
        .take()
        .expect("stdin")
        .write_all(&noncanonical)
        .expect("write invalid request");
    let rejected = rejected.wait_with_output().expect("rejected output");
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(rejected.stderr.len() < 1_024);
}
