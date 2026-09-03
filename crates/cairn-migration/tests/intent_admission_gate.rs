use std::{fs, path::Path};

use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, IntentDecisionMaterialV1, IntentHypothesisSetProposalV1,
    IntentRecoveryInputV1, IntentRecoveryRequestV1, SirCallerClaimId, SirCapabilityManifestV1,
    SirHypothesisId, SirProposalSubmissionV1, SirTaskBundleArtifact, SirTaskLimits,
    derive_user_intent_decision_requests,
};
use cairn_migration::{
    TaskIntentAuthoritySubject, UserIntentAuthorityGrantV1, UserIntentAuthorityScopeV1,
    UserIntentDecisionResponseV1, UserIntentDecisionV1, promote_user_intent,
};
use cairn_protocol::{ContentId, EpisodeId, TaskId};
use serde_json::json;

fn proposal_submission() -> SirProposalSubmissionV1 {
    serde_json::from_value(json!({
        "schema_version": 1,
        "observed_facts": [{
            "id": "atomic-slots",
            "statement": "Output slots are allocated atomically.",
            "citations": [{"path": "src/compact_above.cu", "start_line": 16, "end_line": 20}]
        }],
        "hypotheses": [
            {
                "id": "order-unspecified",
                "layer": "observable-contract",
                "claim": "Any permutation of qualifying values is acceptable.",
                "domain": "Successful calls with sufficient capacity.",
                "supporting_evidence": [{"source": "caller-claim", "claim": "copies-strictly-above"}],
                "counter_evidence": []
            },
            {
                "id": "stable-order",
                "layer": "observable-contract",
                "claim": "Qualifying values retain input-relative order.",
                "domain": "Successful calls with sufficient capacity.",
                "supporting_evidence": [{"source": "caller-claim", "claim": "copies-strictly-above"}],
                "counter_evidence": [{"source": "observed-fact", "observation": "atomic-slots"}]
            }
        ],
        "conflicts": [{
            "id": "order-conflict",
            "statement": "The output-order contracts conflict.",
            "claims": [
                {"source": "hypothesis", "hypothesis": "order-unspecified"},
                {"source": "hypothesis", "hypothesis": "stable-order"}
            ],
            "evidence": [{"source": "observed-fact", "observation": "atomic-slots"}]
        }],
        "unknowns": [{
            "id": "output-order",
            "kind": "desired-semantics",
            "question": "Must output preserve input-relative order?",
            "evidence": [{"source": "observed-fact", "observation": "atomic-slots"}]
        }],
        "invariants": [{
            "id": "copied-values",
            "statement": "Every output value comes from input.",
            "evidence": [{"source": "caller-claim", "claim": "copies-strictly-above"}]
        }],
        "optimization_freedoms": [],
        "source_dispositions": [],
        "disambiguation_experiments": [{
            "id": "decide-order",
            "targets": [
                {"kind": "conflict", "conflict": "order-conflict"},
                {"kind": "unknown", "unknown": "output-order"}
            ],
            "plan": "Ask the actual task authority whether output ordering is observable.",
            "predictions": [
                "Stable use selects stable-order.",
                "Order-insensitive use selects order-unspecified."
            ]
        }]
    }))
    .expect("strict proposal submission")
}

fn multi_decision_proposal_submission() -> SirProposalSubmissionV1 {
    let mut value = serde_json::to_value(proposal_submission()).expect("proposal json");
    let hypotheses = value["hypotheses"].as_array_mut().expect("hypotheses");
    hypotheses.extend([
        json!({
            "id":"exact-values","layer":"numerical",
            "claim":"Successful outputs must be bitwise exact.",
            "domain":"Successful calls with sufficient capacity.",
            "supporting_evidence":[{"source":"caller-claim","claim":"copies-strictly-above"}],
            "counter_evidence":[]
        }),
        json!({
            "id":"rounded-values","layer":"numerical",
            "claim":"Successful outputs may use implementation-defined rounding.",
            "domain":"Successful calls with sufficient capacity.",
            "supporting_evidence":[{"source":"observed-fact","observation":"atomic-slots"}],
            "counter_evidence":[]
        }),
    ]);
    hypotheses.sort_by_key(|item| item["id"].as_str().expect("hypothesis id").to_owned());
    value["conflicts"]
        .as_array_mut()
        .expect("conflicts")
        .push(json!({
            "id":"rounding-conflict",
            "statement":"The exact and rounded numerical contracts conflict.",
            "claims":[
                {"source":"hypothesis","hypothesis":"exact-values"},
                {"source":"hypothesis","hypothesis":"rounded-values"}
            ],
            "evidence":[{"source":"observed-fact","observation":"atomic-slots"}]
        }));
    value["conflicts"]
        .as_array_mut()
        .expect("conflicts")
        .sort_by_key(|item| item["id"].as_str().expect("conflict id").to_owned());
    value["unknowns"]
        .as_array_mut()
        .expect("unknowns")
        .push(json!({
            "id":"rounding-policy","kind":"desired-semantics",
            "question":"Must successful output values be bitwise exact?",
            "evidence":[{"source":"observed-fact","observation":"atomic-slots"}]
        }));
    value["disambiguation_experiments"]
        .as_array_mut()
        .expect("experiments")
        .push(json!({
            "id":"decide-rounding",
            "targets":[
                {"kind":"conflict","conflict":"rounding-conflict"},
                {"kind":"unknown","unknown":"rounding-policy"}
            ],
            "plan":"Ask the task authority whether bitwise equality is observable.",
            "predictions":["Exact equality selects exact-values.","Tolerance selects rounded-values."]
        }));
    value["disambiguation_experiments"]
        .as_array_mut()
        .expect("experiments")
        .sort_by_key(|item| item["id"].as_str().expect("experiment id").to_owned());
    serde_json::from_value(value).expect("strict multi-decision proposal")
}

#[test]
fn workflow_function_promotes_only_an_exact_authorized_decision() {
    let caller_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let recovery_request: IntentRecoveryRequestV1 =
        serde_json::from_slice(&fs::read(caller_path).expect("caller request"))
            .expect("strict recovery request");
    let task_id = TaskId::new();
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        ContentId::<SirTaskBundleArtifact>::derive(b"in-process gate test bundle")
            .expect("bundle identity"),
        recovery_request,
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery input");
    let recovery_id = recovery.identity().expect("recovery identity");
    let proposal: IntentHypothesisSetProposalV1 = serde_json::from_value(json!({
        "schema_version": 1,
        "recovery_input": recovery_id,
        "episode_id": EpisodeId::new(),
        "model_configuration": ContentId::<AgentResolvedRuntimeModelArtifact>::derive(b"test model")
            .expect("model identity"),
        "submission": proposal_submission(),
    }))
    .expect("proposal envelope");
    let proposal_id = proposal.identity().expect("proposal identity");
    let requests =
        derive_user_intent_decision_requests(proposal_id, &proposal, recovery_id, &recovery)
            .expect("decision requests");
    let request = &requests.requests()[0];
    let request_id = request.identity().expect("request identity");
    let grant = UserIntentAuthorityGrantV1::new(
        task_id,
        TaskIntentAuthoritySubject::new("task-authority:user").expect("authority subject"),
        UserIntentAuthorityScopeV1::new(vec![
            SirCallerClaimId::new("copies-strictly-above").expect("claim"),
            SirCallerClaimId::new("output-capacity").expect("claim"),
        ])
        .expect("authority scope"),
    );
    let grant_id = grant.identity().expect("grant identity");
    let decision = UserIntentDecisionV1::new(
        request_id,
        grant_id,
        UserIntentDecisionResponseV1::SelectHypothesis {
            hypothesis: SirHypothesisId::new("order-unspecified").expect("hypothesis"),
        },
    );
    let prepared = promote_user_intent(
        proposal_id,
        &proposal,
        recovery_id,
        &recovery,
        &requests,
        &[IntentDecisionMaterialV1 {
            request,
            grant: &grant,
            decision: &decision,
        }],
    )
    .expect("in-process Intent Admission");

    assert_eq!(prepared.public_outcome().contract().task_id(), task_id);
    assert_eq!(prepared.public_outcome().contract().proposal(), proposal_id);
    assert_eq!(
        prepared.public_outcome().contract().decisions()[0].request(),
        request_id
    );
}

#[test]
fn promotion_requires_and_preserves_every_administrator_decision() {
    let caller_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cuda-ascend/sir/compact-above-f32/v1/caller-intent.json");
    let recovery_request: IntentRecoveryRequestV1 =
        serde_json::from_slice(&fs::read(caller_path).expect("caller request"))
            .expect("strict recovery request");
    let task_id = TaskId::new();
    let recovery = IntentRecoveryInputV1::new(
        task_id,
        ContentId::<SirTaskBundleArtifact>::derive(b"multi-decision task bundle")
            .expect("bundle identity"),
        recovery_request,
        SirCapabilityManifestV1::proposal_only(SirTaskLimits::default()),
    )
    .expect("recovery input");
    let recovery_id = recovery.identity().expect("recovery identity");
    let proposal: IntentHypothesisSetProposalV1 = serde_json::from_value(json!({
        "schema_version":1,
        "recovery_input":recovery_id,
        "episode_id":EpisodeId::new(),
        "model_configuration":ContentId::<AgentResolvedRuntimeModelArtifact>::derive(b"test model")
            .expect("model identity"),
        "submission":multi_decision_proposal_submission(),
    }))
    .expect("proposal envelope");
    let proposal_id = proposal.identity().expect("proposal identity");
    let requests =
        derive_user_intent_decision_requests(proposal_id, &proposal, recovery_id, &recovery)
            .expect("decision requests");
    assert_eq!(requests.requests().len(), 2);
    let mut grants = Vec::new();
    let mut decisions = Vec::new();
    for request in requests.requests() {
        let grant = UserIntentAuthorityGrantV1::new(
            task_id,
            TaskIntentAuthoritySubject::new("task-authority:user").expect("authority subject"),
            UserIntentAuthorityScopeV1::new(vec![
                SirCallerClaimId::new("copies-strictly-above").expect("claim"),
                SirCallerClaimId::new("output-capacity").expect("claim"),
            ])
            .expect("authority scope"),
        );
        let hypothesis = request.options()[0].hypothesis().clone();
        let decision = UserIntentDecisionV1::new(
            request.identity().expect("request identity"),
            grant.identity().expect("grant identity"),
            UserIntentDecisionResponseV1::SelectHypothesis { hypothesis },
        );
        grants.push(grant);
        decisions.push(decision);
    }
    let materials = requests
        .requests()
        .iter()
        .zip(&grants)
        .zip(&decisions)
        .map(|((request, grant), decision)| IntentDecisionMaterialV1 {
            request,
            grant,
            decision,
        })
        .collect::<Vec<_>>();
    let prepared = promote_user_intent(
        proposal_id,
        &proposal,
        recovery_id,
        &recovery,
        &requests,
        &materials,
    )
    .expect("complete decision set");
    assert_eq!(prepared.public_outcome().contract().decisions().len(), 2);
    assert_eq!(
        prepared.public_outcome().contract().admitted_claims().len(),
        2
    );
    assert!(
        promote_user_intent(
            proposal_id,
            &proposal,
            recovery_id,
            &recovery,
            &requests,
            &materials[..1],
        )
        .is_err()
    );
}

/// Every task fixture has to be readable by the entry point that will be asked to read it.
///
/// A caller declaration that only fails to parse at submission time turns an authoring mistake
/// into a runtime failure on a live deployment, where it costs a provider call to discover.
#[test]
fn every_task_fixture_carries_a_readable_caller_declaration() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cuda-ascend/sir");
    let mut read = 0_usize;
    for task in std::fs::read_dir(&root).expect("task fixture root") {
        let declaration = task
            .expect("task entry")
            .path()
            .join("v1/caller-intent.json");
        if !declaration.is_file() {
            continue;
        }
        let bytes = std::fs::read(&declaration).expect("caller declaration bytes");
        serde_json::from_slice::<cairn_migration::IntentRecoveryRequestV1>(&bytes).unwrap_or_else(
            |error| {
                panic!(
                    "{} is not a readable declaration: {error}",
                    declaration.display()
                )
            },
        );
        read += 1;
    }
    assert!(
        read >= 2,
        "expected every task fixture to be scanned, saw {read}"
    );
}
