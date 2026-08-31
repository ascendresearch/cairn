use std::{fs, path::Path};

use cairn_migration::{
    AgentResolvedRuntimeModelArtifact, IntentHypothesisSetProposalV1, IntentRecoveryInputV1,
    IntentRecoveryRequestV1, SirCallerClaimId, SirCapabilityManifestV1, SirHypothesisId,
    SirProposalSubmissionV1, SirTaskBundleArtifact, SirTaskLimits,
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
        request_id,
        request,
        grant_id,
        &grant,
        decision.identity().expect("decision identity"),
        &decision,
    )
    .expect("in-process Intent Admission");

    assert_eq!(prepared.public_outcome().contract().task_id(), task_id);
    assert_eq!(prepared.public_outcome().contract().proposal(), proposal_id);
    assert_eq!(prepared.public_outcome().contract().request(), request_id);
}
