use std::{fs, io::Cursor, path::Path, process::Command};

use cairn_admission::{
    IntentAdmissionPublicOutcomeV1, TaskIntentAuthoritySubject, UserIntentAuthorityGrantArtifact,
    UserIntentAuthorityGrantV1, UserIntentAuthorityScopeV1, UserIntentDecisionArtifact,
    UserIntentDecisionResponseV1, UserIntentDecisionV1, derive_collection_output_oracle_decision,
};
use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, AuthoritativeIntentClaimV1, CollectionOracleElementArtifact,
    CollectionOutputComparisonV1, CollectionOutputIntentV1, CollectionOutputOraclePolicyV1,
    CollectionOutputOrderContractV1, CollectionReportedCount, ExpectedCollectionOracleOutputV1,
    IntentDecisionRequestBatchV1, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, ObservedCollectionOracleOutputV1, SirCallerClaimId,
    SirCapabilityManifestV1, SirHypothesisId, SirTaskBundleArtifact, SirTaskLimits,
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
    TaskId,
) {
    let caller_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let request: IntentRecoveryRequestV1 =
        serde_json::from_slice(&fs::read(caller_path).expect("caller request"))
            .expect("strict request");
    let task_id = TaskId::new();
    let input = IntentRecoveryInputV1::new(
        task_id,
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
        "model_configuration":ContentId::<AgentResolvedRuntimeModelArtifact>::derive(b"recorded model").expect("model"),
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
    (proposal_id, input_id, task_id)
}

#[test]
#[allow(clippy::too_many_lines)]
fn child_process_reads_exact_public_artifacts_and_emits_only_canonical_v1() {
    let state = tempfile::tempdir().expect("state");
    let database = state.path().join("content.db");
    let cas = state.path().join("cas");
    let (proposal_id, input_id, task_id) = archive_input_and_proposal(&database, &cas);
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

    let request = batch.requests()[0].clone();
    let request_id = request.identity().expect("request identity");
    let selection_claim = SirCallerClaimId::new("copies-strictly-above").expect("caller claim");
    let grant = UserIntentAuthorityGrantV1::new(
        task_id,
        TaskIntentAuthoritySubject::new("task-authority:user").expect("authority subject"),
        UserIntentAuthorityScopeV1::CollectionOutput {
            selection_claim: selection_claim.clone(),
        },
    );
    let grant_id = grant.identity().expect("grant identity");
    let decision = UserIntentDecisionV1::new(
        request_id,
        grant_id,
        UserIntentDecisionResponseV1::SelectHypothesis {
            hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
            authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                CollectionOutputIntentV1::exact_selected_occurrences(
                    selection_claim,
                    CollectionOutputOrderContractV1::UnspecifiedPermutation,
                ),
            ),
        },
    );
    let decision_id = decision.identity().expect("decision identity");
    let mut controller_store = SqliteContentStore::open(&database, &cas).expect("controller store");
    assert_eq!(
        controller_store
            .put::<cairn_migration::UserIntentDecisionRequestArtifact>(&mut Cursor::new(
                cairn_codec::to_vec(&request).expect("request bytes"),
            ))
            .expect("archive request")
            .content_id,
        request_id
    );
    assert_eq!(
        controller_store
            .put::<UserIntentAuthorityGrantArtifact>(&mut Cursor::new(
                cairn_codec::to_vec(&grant).expect("grant bytes"),
            ))
            .expect("archive grant")
            .content_id,
        grant_id
    );
    assert_eq!(
        controller_store
            .put::<UserIntentDecisionArtifact>(&mut Cursor::new(
                cairn_codec::to_vec(&decision).expect("decision bytes"),
            ))
            .expect("archive decision")
            .content_id,
        decision_id
    );
    drop(controller_store);

    let restricted_database = state.path().join("restricted.db");
    let restricted_cas = state.path().join("restricted-cas");
    let promoted = Command::new(env!("CARGO_BIN_EXE_cairn-admission"))
        .args([
            "promote-user-intent",
            database.to_str().expect("database path"),
            cas.to_str().expect("CAS path"),
            restricted_database.to_str().expect("restricted database"),
            restricted_cas.to_str().expect("restricted CAS"),
            &decision_id.to_wire(),
        ])
        .env_clear()
        .output()
        .expect("promotion process");
    assert!(
        promoted.status.success(),
        "{}",
        String::from_utf8_lossy(&promoted.stderr)
    );
    assert!(promoted.stderr.is_empty());
    let outcome: IntentAdmissionPublicOutcomeV1 =
        cairn_codec::from_slice(&promoted.stdout).expect("public admitted outcome");
    assert_eq!(
        cairn_codec::to_vec(&outcome).expect("canonical outcome"),
        promoted.stdout
    );
    let oracle = derive_collection_output_oracle_decision(&outcome).expect("Oracle policy");
    assert_eq!(
        oracle.policy(),
        CollectionOutputOraclePolicyV1::ExactMultisetAndCount
    );
    let first = ContentId::<CollectionOracleElementArtifact>::derive(b"first").expect("first");
    let second = ContentId::<CollectionOracleElementArtifact>::derive(b"second").expect("second");
    let expected =
        ExpectedCollectionOracleOutputV1::new(vec![first, second]).expect("expected output");
    let reordered =
        ObservedCollectionOracleOutputV1::new(vec![second, first], CollectionReportedCount::new(2))
            .expect("reordered output");
    assert_eq!(
        oracle.compare(&expected, &reordered),
        CollectionOutputComparisonV1::Equivalent
    );
    let missing =
        ObservedCollectionOracleOutputV1::new(vec![first], CollectionReportedCount::new(1))
            .expect("missing output");
    assert_eq!(
        oracle.compare(&expected, &missing),
        CollectionOutputComparisonV1::ReportedCountMismatch
    );
    let wrong_count =
        ObservedCollectionOracleOutputV1::new(vec![first, second], CollectionReportedCount::new(1))
            .expect("wrong count output");
    assert_eq!(
        oracle.compare(&expected, &wrong_count),
        CollectionOutputComparisonV1::ReportedCountMismatch
    );
    let duplicate =
        ObservedCollectionOracleOutputV1::new(vec![first, first], CollectionReportedCount::new(2))
            .expect("duplicate output");
    assert_eq!(
        oracle.compare(&expected, &duplicate),
        CollectionOutputComparisonV1::ElementMultisetMismatch
    );

    let restricted =
        SqliteContentStore::open_immutable_read_only(&restricted_database, &restricted_cas)
            .expect("restricted store");
    let mut archived_contract = Vec::new();
    restricted
        .write_to(
            &outcome.contract().identity().expect("contract identity"),
            &mut archived_contract,
        )
        .expect("restricted contract");
    let _: cairn_admission::MigrationIntentContractV1 =
        cairn_codec::from_slice(&archived_contract).expect("strict archived contract");
    let mut archived_decision = Vec::new();
    restricted
        .write_to(&outcome.restricted_decision(), &mut archived_decision)
        .expect("restricted decision");
    let _: cairn_admission::RestrictedIntentAdmissionDecisionV1 =
        cairn_codec::from_slice(&archived_decision).expect("strict restricted decision");

    let unoffered = UserIntentDecisionV1::new(
        request_id,
        grant_id,
        UserIntentDecisionResponseV1::SelectHypothesis {
            hypothesis: SirHypothesisId::new("not-an-offered-hypothesis").expect("hypothesis"),
            authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                CollectionOutputIntentV1::exact_selected_occurrences(
                    SirCallerClaimId::new("copies-strictly-above").expect("caller claim"),
                    CollectionOutputOrderContractV1::UnspecifiedPermutation,
                ),
            ),
        },
    );
    let unoffered_id = unoffered.identity().expect("unoffered decision identity");
    let mut controller_store = SqliteContentStore::open(&database, &cas).expect("controller store");
    assert_eq!(
        controller_store
            .put::<UserIntentDecisionArtifact>(&mut Cursor::new(
                cairn_codec::to_vec(&unoffered).expect("unoffered bytes"),
            ))
            .expect("archive unoffered decision")
            .content_id,
        unoffered_id
    );
    drop(controller_store);
    let rejected_restricted_database = state.path().join("rejected-restricted.db");
    let rejected = Command::new(env!("CARGO_BIN_EXE_cairn-admission"))
        .args([
            "promote-user-intent",
            database.to_str().expect("database path"),
            cas.to_str().expect("CAS path"),
            rejected_restricted_database
                .to_str()
                .expect("rejected restricted database"),
            state
                .path()
                .join("rejected-restricted-cas")
                .to_str()
                .expect("rejected restricted CAS"),
            &unoffered_id.to_wire(),
        ])
        .env_clear()
        .output()
        .expect("rejected promotion process");
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert!(!rejected_restricted_database.exists());

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
