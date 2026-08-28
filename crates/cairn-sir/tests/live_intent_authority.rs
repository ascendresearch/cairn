use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    process::{Command, Stdio},
    str::FromStr,
};

use cairn_admission::{
    AuthoritativeIntentClaimV1, CollectionOracleElementArtifact, CollectionOutputComparisonV1,
    CollectionOutputIntentV1, CollectionOutputOrderContractV1, CollectionReportedCount,
    ExpectedCollectionOracleOutputV1, ObservedCollectionOracleOutputV1, TaskIntentAuthoritySubject,
    UserIntentAuthorityGrantArtifact, UserIntentAuthorityGrantV1, UserIntentAuthorityScopeV1,
    UserIntentDecisionArtifact, UserIntentDecisionResponseV1, UserIntentDecisionV1,
    derive_collection_output_oracle_decision, promote_user_intent,
};
use cairn_migration::{
    IntentHypothesisSetProposalV1, IntentRecoveryInputArtifact, IntentRecoveryInputV1,
    SirCallerClaimId, SirHypothesisId, SirIntentHypothesisSetProposalArtifact, SirProcessRequestV1,
    SirProcessTerminalV1, SirTaskBundleArtifact, SirTaskBundleV1,
    UserIntentDecisionRequestArtifact, derive_user_intent_decision_requests,
};
use cairn_protocol::{ContentId, ContentType, OperationId, SirRunId};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use serde::de::DeserializeOwned;

#[test]
#[ignore = "requires the private DEV-006 live run store; makes no provider call"]
#[allow(clippy::too_many_lines)]
fn exact_live_proposal_crosses_process_and_drives_first_admitted_oracle_policy() {
    let run_root = std::env::var("CAIRN_DEV006_RUN_ROOT").expect("DEV-006 run root");
    let proposal_id = ContentId::<SirIntentHypothesisSetProposalArtifact>::from_str(
        &std::env::var("CAIRN_DEV006_PROPOSAL_ID").expect("DEV-006 proposal ID"),
    )
    .expect("typed proposal ID");
    let store = SqliteContentStore::open_immutable_read_only(
        Path::new(&run_root).join("content.db"),
        Path::new(&run_root).join("cas"),
    )
    .expect("live run store");
    let proposal: IntentHypothesisSetProposalV1 = load(&store, &proposal_id);
    let recovery_input_id = proposal.recovery_input();
    let recovery_input: IntentRecoveryInputV1 = load(&store, &recovery_input_id);
    let task_bundle: SirTaskBundleV1 =
        load::<SirTaskBundleArtifact, _>(&store, &recovery_input.task_bundle());
    let process_request = SirProcessRequestV1::new(
        SirRunId::new(),
        OperationId::new(),
        task_bundle.clone(),
        recovery_input.clone(),
        proposal.clone(),
    )
    .expect("exact SIR process request");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cairn-sir"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("isolated SIR process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(&cairn_codec::to_vec(&process_request).expect("request bytes"))
        .expect("write request");
    let output = child.wait_with_output().expect("terminal output");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let terminal: SirProcessTerminalV1 =
        cairn_codec::from_slice(&output.stdout).expect("strict process terminal");
    assert_eq!(terminal.proposal_id(), proposal_id);

    let batch = derive_user_intent_decision_requests(
        proposal_id,
        terminal.proposal(),
        recovery_input_id,
        &recovery_input,
    )
    .expect("exact live decision request");
    assert_eq!(batch.requests().len(), 1);
    let request = &batch.requests()[0];
    let request_id = request.identity().expect("request identity");
    let chosen =
        SirHypothesisId::new("h-compact-set-order-unspecified").expect("chosen hypothesis");
    assert!(
        request
            .options()
            .iter()
            .any(|option| option.hypothesis() == &chosen)
    );
    let selection_claim = SirCallerClaimId::new("copies-strictly-above").expect("selection claim");
    let grant = UserIntentAuthorityGrantV1::new(
        recovery_input.task_id(),
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
            hypothesis: chosen,
            authoritative_claim: AuthoritativeIntentClaimV1::CollectionOutput(
                CollectionOutputIntentV1::exact_selected_occurrences(
                    selection_claim,
                    CollectionOutputOrderContractV1::UnspecifiedPermutation,
                ),
            ),
        },
    );
    let decision_id = decision.identity().expect("decision identity");
    if let Ok(smoke_root) = std::env::var("CAIRN_DEV008_SMOKE_ROOT") {
        export_principal_smoke_state(
            Path::new(&smoke_root),
            &process_request,
            &task_bundle,
            &recovery_input,
            proposal_id,
            &proposal,
            request,
            &grant,
            &decision,
        );
    }
    let prepared = promote_user_intent(
        proposal_id,
        terminal.proposal(),
        recovery_input_id,
        &recovery_input,
        request_id,
        request,
        grant_id,
        &grant,
        decision_id,
        &decision,
    )
    .expect("first exact promotion");
    let oracle = derive_collection_output_oracle_decision(prepared.public_outcome())
        .expect("contract-only Oracle decision");
    let first = ContentId::<CollectionOracleElementArtifact>::derive(b"selected-a").expect("first");
    let second =
        ContentId::<CollectionOracleElementArtifact>::derive(b"selected-b").expect("second");
    let expected = ExpectedCollectionOracleOutputV1::new(vec![first, second]).expect("expected");
    let reordered =
        ObservedCollectionOracleOutputV1::new(vec![second, first], CollectionReportedCount::new(2))
            .expect("reordered");
    assert_eq!(
        oracle.compare(&expected, &reordered),
        CollectionOutputComparisonV1::Equivalent
    );
}

#[allow(clippy::too_many_arguments)]
fn export_principal_smoke_state(
    root: &Path,
    process_request: &SirProcessRequestV1,
    task_bundle: &SirTaskBundleV1,
    recovery_input: &IntentRecoveryInputV1,
    proposal_id: ContentId<SirIntentHypothesisSetProposalArtifact>,
    proposal: &IntentHypothesisSetProposalV1,
    request: &cairn_migration::UserIntentDecisionRequestV1,
    grant: &UserIntentAuthorityGrantV1,
    decision: &UserIntentDecisionV1,
) {
    fs::create_dir_all(root).expect("smoke root");
    fs::write(
        root.join("sir-request.json"),
        cairn_codec::to_vec(process_request).expect("process request bytes"),
    )
    .expect("write process request");
    let public = root.join("public");
    fs::create_dir_all(&public).expect("public Controller root");
    let mut store = SqliteContentStore::open(public.join("content.db"), public.join("cas"))
        .expect("public Controller store");
    archive::<SirTaskBundleArtifact>(&mut store, task_bundle);
    archive::<IntentRecoveryInputArtifact>(&mut store, recovery_input);
    assert_eq!(
        archive::<SirIntentHypothesisSetProposalArtifact>(&mut store, proposal),
        proposal_id
    );
    archive::<UserIntentDecisionRequestArtifact>(&mut store, request);
    archive::<UserIntentAuthorityGrantArtifact>(&mut store, grant);
    let decision_id = archive::<UserIntentDecisionArtifact>(&mut store, decision);
    fs::write(root.join("decision-id"), decision_id.to_wire()).expect("write decision ID");
}

fn archive<T: ContentType>(
    store: &mut SqliteContentStore,
    value: &impl serde::Serialize,
) -> ContentId<T> {
    store
        .put::<T>(&mut Cursor::new(
            cairn_codec::to_vec(value).expect("canonical archive bytes"),
        ))
        .expect("archive content")
        .content_id
}

fn load<T, V>(store: &SqliteContentStore, content_id: &ContentId<T>) -> V
where
    T: ContentType,
    V: DeserializeOwned,
{
    let mut bytes = Vec::new();
    store
        .write_to(content_id, &mut bytes)
        .expect("content bytes");
    cairn_codec::from_slice(&bytes).expect("strict typed content")
}
