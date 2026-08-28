use std::{fs, path::PathBuf, process::Command};

use cairn_testkit::fixtures::{
    IntentQualificationArtifactIdentity, IntentQualificationArtifactPath,
    IntentQualificationBundleIdentity, IntentQualificationControlReviewReceiptV1,
    IntentQualificationReviewSubjectIdentity, QualificationControlReviewReceiptId,
    QualificationMechanismSlot, RestrictedQualificationManifestId,
    decode_intent_mechanism_contracts_v1, decode_intent_qualification_control_review_receipt_v1,
    decode_intent_qualification_control_suite_v1, decode_intent_qualification_manifest_v1,
    decode_intent_qualification_review_assignments_v1, decode_intent_requalification_plans_v1,
    decode_intent_restricted_qualification_summary_v1, scan_public_tree,
    validate_intent_qualification_freeze_transition,
};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn qualification_root() -> PathBuf {
    repository_root().join("fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1")
}

fn read(name: &str) -> Vec<u8> {
    fs::read(qualification_root().join(name)).expect("DEV-002 public artifact")
}

fn control_paths() -> [&'static str; 10] {
    [
        "controls/01-strict-v1-identity.json",
        "controls/02-abi-static-facts.json",
        "controls/03-required-evidence.json",
        "controls/04-recipe-plan-validation.json",
        "controls/05-recorded-host-runner.json",
        "controls/06-raw-observation-comparison.json",
        "controls/07-receipt-closure.json",
        "controls/08-intent-policy.json",
        "controls/09-intent-gate.json",
        "controls/10-diagnostic-redaction.json",
    ]
}

#[test]
fn public_bundle_is_canonical_complete_and_bound_to_dev001() {
    let manifest_bytes = read("manifest.json");
    let manifest =
        decode_intent_qualification_manifest_v1(&manifest_bytes).expect("strict manifest");
    assert_eq!(manifest.asset_count(), 15);
    manifest
        .validate_tree(&repository_root())
        .expect("identity-bound qualification tree");
    assert!(IntentQualificationBundleIdentity::derive(&manifest_bytes).is_ok());

    let contracts = decode_intent_mechanism_contracts_v1(&read("mechanism-contracts.json"))
        .expect("strict mechanism contracts");
    assert_eq!(contracts.contract_count(), 10);
    assert_eq!(
        decode_intent_qualification_review_assignments_v1(&read("review-assignments.json"))
            .expect("strict review assignments")
            .assignment_count(),
        10
    );
    assert_eq!(
        decode_intent_requalification_plans_v1(&read("requalification-plans.json"))
            .expect("strict requalification plans")
            .plan_count(),
        10
    );

    for ((path, slot), expected_slot) in control_paths()
        .into_iter()
        .zip(QualificationMechanismSlot::all())
        .zip(QualificationMechanismSlot::all())
    {
        assert_eq!(slot, expected_slot);
        let suite = decode_intent_qualification_control_suite_v1(&read(path))
            .expect("strict qualification control suite");
        assert_eq!(suite.slot(), slot);
        assert!(suite.case_count() >= 5);
        let categories = suite.categories();
        assert!(
            contracts
                .required_categories_for(slot)
                .iter()
                .all(|category| categories.contains(category))
        );
    }

    let restricted =
        decode_intent_restricted_qualification_summary_v1(&read("restricted-controls.public.json"))
            .expect("strict redacted restricted summary");
    assert_eq!(restricted.control_count(), 3);
    assert!(restricted.is_review_pending());
    assert!(!restricted.is_frozen_reviewed());
}

#[test]
fn strict_decoders_reject_non_v1_unknown_and_fake_qualification_fields() {
    let contracts = read("mechanism-contracts.json");
    let non_v1 = String::from_utf8(contracts.clone())
        .expect("contract text")
        .replace("\"schema_version\":1", "\"schema_version\":2");
    assert!(decode_intent_mechanism_contracts_v1(non_v1.as_bytes()).is_err());

    let unknown = String::from_utf8(contracts.clone())
        .expect("contract text")
        .replacen('{', "{\"implementation_identity\":\"not-created\",", 1);
    assert!(decode_intent_mechanism_contracts_v1(unknown.as_bytes()).is_err());

    let fake_receipt = String::from_utf8(contracts)
        .expect("contract text")
        .replacen('{', "{\"qualification_receipt\":\"not-created\",", 1);
    assert!(decode_intent_mechanism_contracts_v1(fake_receipt.as_bytes()).is_err());

    let suite = read("controls/09-intent-gate.json");
    let mut noncanonical = suite.clone();
    noncanonical.push(b'\n');
    assert!(decode_intent_qualification_control_suite_v1(&noncanonical).is_err());

    let incomplete = String::from_utf8(suite)
        .expect("suite text")
        .replace(
            ",{\"category\":\"constructor-bypass\",\"control_id\":\"gate-constructor-bypass\",\"expected\":\"admission-blocked\",\"stimulus\":\"constructor-bypass-attempt\"}",
            "",
        );
    assert!(decode_intent_qualification_control_suite_v1(incomplete.as_bytes()).is_err());
}

#[test]
fn qualification_public_path_boundary_fails_closed() {
    assert!(
        IntentQualificationArtifactPath::new(
            "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/mechanism-contracts.json"
        )
        .is_ok()
    );
    assert!(IntentQualificationArtifactPath::new("/absolute/control.json").is_err());
    assert!(
        IntentQualificationArtifactPath::new(
            "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/../control.json"
        )
        .is_err()
    );
    assert!(
        IntentQualificationArtifactPath::new(
            "fixtures/cuda-ascend/qualification/intent/reduce-sum-f32/v1/restricted/control.json"
        )
        .is_err()
    );
}

#[test]
fn public_tree_passes_shared_sanitation_and_private_tree_is_untracked() {
    let root = repository_root();
    let profile = fs::read(root.join("fixtures/regressions/v1/sanitation-scan-profile.json"))
        .expect("shared scan profile");
    let report = scan_public_tree(&root, &qualification_root(), &profile)
        .expect("shared public qualification scan");
    assert!(report.is_clean(), "findings: {:?}", report.findings());

    if root.join(".git").exists() {
        let output = Command::new("git")
            .args(["ls-files", ".cairn"])
            .current_dir(&root)
            .output()
            .expect("git ls-files");
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn control_review_receipt_is_distinct_and_fail_closed() {
    let review_subject = IntentQualificationReviewSubjectIdentity::derive(b"public review subject")
        .expect("review subject");
    let private_manifest = RestrictedQualificationManifestId::derive(b"private control manifest")
        .expect("private manifest");
    let receipt = format!(
        "{{\"checks\":[\"golden-independence\",\"wrong-binding-validity\",\"redaction-canary-validity\",\"exposure-and-diagnostic-safety\"],\"control_author\":\"qualification-control-author-ws-quality\",\"control_manifest_identity\":\"{private_manifest}\",\"controls\":[\"wrong-binding\",\"hidden-redaction-canary\",\"secret-redaction-canary\"],\"outcome\":\"accepted\",\"review_subject_identity\":\"{review_subject}\",\"reviewer\":\"qualification-reviewer-user\",\"schema_version\":1}}"
    );
    let decoded = decode_intent_qualification_control_review_receipt_v1(receipt.as_bytes())
        .expect("strict control-review receipt");
    assert_eq!(decoded.review_subject_identity(), review_subject);
    assert_eq!(decoded.control_manifest_identity(), private_manifest);
    assert_eq!(decoded.reviewer().as_str(), "qualification-reviewer-user");
    assert!(IntentQualificationControlReviewReceiptV1::identity(receipt.as_bytes()).is_ok());

    let self_review = receipt.replace(
        "qualification-reviewer-user",
        "qualification-control-author-ws-quality",
    );
    assert!(decode_intent_qualification_control_review_receipt_v1(self_review.as_bytes()).is_err());
    let incomplete = receipt.replace(",\"exposure-and-diagnostic-safety\"", "");
    assert!(decode_intent_qualification_control_review_receipt_v1(incomplete.as_bytes()).is_err());
    let wrong_domain = IntentQualificationArtifactIdentity::derive(b"public review subject")
        .expect("wrong identity domain")
        .to_string();
    let wrong_binding = receipt.replace(&review_subject.to_string(), &wrong_domain);
    assert!(
        decode_intent_qualification_control_review_receipt_v1(wrong_binding.as_bytes()).is_err()
    );
}

#[test]
fn synthetic_freeze_transition_allows_only_review_authority_projection() {
    let review_manifest_bytes = read("manifest.json");
    let review_subject = IntentQualificationReviewSubjectIdentity::derive(&review_manifest_bytes)
        .expect("review subject");
    let private_manifest =
        RestrictedQualificationManifestId::derive(b"synthetic exact private controls")
            .expect("private manifest");
    let receipt = format!(
        "{{\"checks\":[\"golden-independence\",\"wrong-binding-validity\",\"redaction-canary-validity\",\"exposure-and-diagnostic-safety\"],\"control_author\":\"qualification-control-author-ws-quality\",\"control_manifest_identity\":\"{private_manifest}\",\"controls\":[\"wrong-binding\",\"hidden-redaction-canary\",\"secret-redaction-canary\"],\"outcome\":\"accepted\",\"review_subject_identity\":\"{review_subject}\",\"reviewer\":\"qualification-reviewer-user\",\"schema_version\":1}}"
    );
    let receipt_id = IntentQualificationControlReviewReceiptV1::identity(receipt.as_bytes())
        .expect("receipt identity");
    let accepted_summary = format!(
        "{{\"controls\":[{{\"kind\":\"wrong-binding\",\"status\":\"frozen-reviewed\"}},{{\"kind\":\"hidden-redaction-canary\",\"status\":\"frozen-reviewed\"}},{{\"kind\":\"secret-redaction-canary\",\"status\":\"frozen-reviewed\"}}],\"review_receipt_identity\":\"{receipt_id}\",\"schema_version\":1}}"
    );
    let accepted_summary =
        decode_intent_restricted_qualification_summary_v1(accepted_summary.as_bytes())
            .expect("accepted summary");
    let accepted_summary_identity =
        IntentQualificationArtifactIdentity::derive(accepted_summary_bytes(&receipt_id).as_bytes())
            .expect("accepted summary identity");
    let pending_summary_identity =
        IntentQualificationArtifactIdentity::derive(&read("restricted-controls.public.json"))
            .expect("pending summary identity");
    let accepted_manifest_bytes = String::from_utf8(review_manifest_bytes.clone())
        .expect("manifest text")
        .replace(
            &pending_summary_identity.to_string(),
            &accepted_summary_identity.to_string(),
        );
    let accepted_manifest =
        decode_intent_qualification_manifest_v1(accepted_manifest_bytes.as_bytes())
            .expect("accepted manifest");

    validate_intent_qualification_freeze_transition(
        &review_manifest_bytes,
        private_manifest,
        &accepted_manifest,
        &accepted_summary,
        receipt.as_bytes(),
    )
    .expect("authority-only freeze transition");

    let wrong_receipt =
        receipt.replace("qualification-reviewer-user", "qualification-reviewer-two");
    assert!(
        validate_intent_qualification_freeze_transition(
            &review_manifest_bytes,
            private_manifest,
            &accepted_manifest,
            &accepted_summary,
            wrong_receipt.as_bytes(),
        )
        .is_err()
    );

    let wrong_private_manifest =
        RestrictedQualificationManifestId::derive(b"wrong-private-manifest")
            .expect("wrong private manifest identity");
    assert!(
        validate_intent_qualification_freeze_transition(
            &review_manifest_bytes,
            wrong_private_manifest,
            &accepted_manifest,
            &accepted_summary,
            receipt.as_bytes(),
        )
        .is_err()
    );
}

fn accepted_summary_bytes(receipt_id: &QualificationControlReviewReceiptId) -> String {
    format!(
        "{{\"controls\":[{{\"kind\":\"wrong-binding\",\"status\":\"frozen-reviewed\"}},{{\"kind\":\"hidden-redaction-canary\",\"status\":\"frozen-reviewed\"}},{{\"kind\":\"secret-redaction-canary\",\"status\":\"frozen-reviewed\"}}],\"review_receipt_identity\":\"{receipt_id}\",\"schema_version\":1}}"
    )
}
