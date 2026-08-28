use std::{fs, path::PathBuf, process::Command};

use cairn_testkit::fixtures::{
    F32Datum, IntentArtifactIdentity, IntentArtifactPath, IntentBundleIdentity,
    IntentReviewSubjectIdentity, ReductionElementCount, RestrictedIntentManifestId,
    decode_intent_claims_v1, decode_intent_manifest_v1, decode_intent_private_review_receipt_v1,
    decode_intent_public_corpus_v1, decode_intent_restricted_summary_v1,
    decode_intent_user_decisions_v1, scan_public_tree,
};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn intent_root() -> PathBuf {
    repository_root().join("fixtures/cuda-ascend/intent/reduce-sum-f32/v1")
}

fn read(name: &str) -> Vec<u8> {
    fs::read(intent_root().join(name)).expect("DEV-001 public artifact")
}

#[test]
fn public_bundle_is_canonical_complete_and_identity_bound() {
    let manifest_bytes = read("manifest.json");
    let manifest = decode_intent_manifest_v1(&manifest_bytes).expect("strict manifest");
    assert_eq!(manifest.asset_count(), 10);
    manifest
        .validate_tree(&repository_root())
        .expect("identity-bound public tree");
    assert!(IntentBundleIdentity::derive(&manifest_bytes).is_ok());

    assert_eq!(
        decode_intent_claims_v1(&read("claims.json"))
            .expect("strict claims")
            .hypothesis_count(),
        4
    );
    assert_eq!(
        decode_intent_public_corpus_v1(&read("public-corpus.json"))
            .expect("strict public corpus")
            .case_count(),
        6
    );
    assert_eq!(
        decode_intent_user_decisions_v1(&read("user-decision-controls.json"))
            .expect("strict user decisions")
            .control_count(),
        5
    );
    let restricted =
        decode_intent_restricted_summary_v1(&read("restricted-partitions.public.json"))
            .expect("strict restricted summary");
    assert_eq!(restricted.partition_count(), 6);
    assert!(restricted.is_frozen_reviewed());
    assert!(restricted.review_receipt_identity().is_some());
}

#[test]
fn strict_decoders_reject_non_v1_noncanonical_and_tampered_inputs() {
    let claims = read("claims.json");
    let mut trailing = claims.clone();
    trailing.push(b'\n');
    assert!(decode_intent_claims_v1(&trailing).is_err());

    let non_v1 = String::from_utf8(claims.clone())
        .expect("claims text")
        .replace("\"schema_version\":1", "\"schema_version\":2");
    assert!(decode_intent_claims_v1(non_v1.as_bytes()).is_err());

    let unknown =
        String::from_utf8(claims)
            .expect("claims text")
            .replacen('{', "{\"admitted\":true,", 1);
    assert!(decode_intent_claims_v1(unknown.as_bytes()).is_err());

    let mut manifest_bytes = read("manifest.json");
    let domain = b"testkit.intent-public-artifact.v1:";
    let offset = manifest_bytes
        .windows(domain.len())
        .position(|window| window == domain)
        .expect("artifact identity domain")
        + domain.len();
    manifest_bytes[offset] = if manifest_bytes[offset] == b'0' {
        b'1'
    } else {
        b'0'
    };
    let manifest = decode_intent_manifest_v1(&manifest_bytes).expect("shape remains valid");
    assert!(manifest.validate_tree(&repository_root()).is_err());
}

#[test]
fn first_domain_and_public_path_boundaries_fail_closed() {
    assert_eq!(ReductionElementCount::new(1).expect("minimum").get(), 1);
    assert_eq!(ReductionElementCount::new(256).expect("maximum").get(), 256);
    assert!(ReductionElementCount::new(0).is_err());
    assert!(ReductionElementCount::new(257).is_err());

    assert!(F32Datum::from_bits(0x0000_0000).is_ok());
    assert!(F32Datum::from_bits(0x8000_0000).is_ok());
    assert!(F32Datum::from_bits(0x4780_0000).is_ok());
    assert!(F32Datum::from_bits(0x4780_0001).is_err());
    assert!(F32Datum::from_bits(0x0000_0001).is_err());
    assert!(F32Datum::from_bits(0x7f80_0000).is_err());
    assert!(F32Datum::from_bits(0x7fc0_0000).is_err());

    assert!(
        IntentArtifactPath::new("fixtures/cuda-ascend/intent/reduce-sum-f32/v1/claims.json")
            .is_ok()
    );
    assert!(IntentArtifactPath::new("/absolute/claims.json").is_err());
    assert!(
        IntentArtifactPath::new("fixtures/cuda-ascend/intent/reduce-sum-f32/v1/../claims.json")
            .is_err()
    );
    assert!(
        IntentArtifactPath::new(
            "fixtures/cuda-ascend/intent/reduce-sum-f32/v1/restricted/case.json"
        )
        .is_err()
    );
}

#[test]
fn source_fixture_exposes_exact_abi_and_no_runtime_claim() {
    let header = String::from_utf8(read("source/include/reduce_sum.h")).expect("header");
    assert!(header.contains(
        "int cairn_reduce_sum_f32(const float* input, float* output, uint32_t element_count);"
    ));
    assert!(!header.contains("cudaStream_t"));

    let launch = String::from_utf8(read("source/src/reduce_sum_launch.cu")).expect("launch");
    assert!(launch.contains("element_count == 0 || element_count > 256"));
    assert!(launch.contains("ranges_overlap(input, output, element_count)"));
    assert!(launch.contains("cairn_reduce_sum_f32_kernel<<<1, 256>>>"));

    let readme = String::from_utf8(read("README.md")).expect("README");
    assert!(readme.contains("no CUDA build or"));
    assert!(readme.contains("restricted case bytes and identities never enter Git"));
}

#[test]
fn public_intent_tree_passes_shared_sanitation_and_private_tree_is_untracked() {
    let root = repository_root();
    let profile = fs::read(root.join("fixtures/regressions/v1/sanitation-scan-profile.json"))
        .expect("shared scan profile");
    let report = scan_public_tree(&root, &intent_root(), &profile).expect("shared public scan");
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
fn private_review_receipt_binds_distinct_exact_inputs_and_independent_reviewer() {
    let review_subject =
        IntentReviewSubjectIdentity::derive(b"synthetic public bundle").expect("review subject");
    let case_set =
        RestrictedIntentManifestId::derive(b"synthetic private case set").expect("case set");
    let receipt = format!(
        "{{\"case_author\":\"cairn-project-ws-domain\",\"case_set_manifest_identity\":\"{case_set}\",\"checks\":[\"clean-room-source-provenance\",\"d039-domain-and-abi\",\"partition-semantic-coverage\",\"public-derivation-independence\",\"binding-tamper-validity\",\"exposure-and-diagnostic-safety\"],\"decision\":\"D-039\",\"outcome\":\"accepted\",\"partitions\":[\"implementation-artifact\",\"source-defect\",\"deployment-quirk\",\"competing-plausible-meaning\",\"genuine-unknown\",\"tamper-wrong-binding\"],\"review_subject_identity\":\"{review_subject}\",\"reviewer\":\"private-reviewer-user\",\"schema_version\":1}}"
    );
    let decoded = decode_intent_private_review_receipt_v1(receipt.as_bytes())
        .expect("strict independent review receipt");
    assert_eq!(decoded.review_subject_identity(), review_subject);
    assert_eq!(decoded.case_set_manifest_identity(), case_set);
    assert_eq!(decoded.reviewer().as_str(), "private-reviewer-user");

    let self_review = receipt.replace("private-reviewer-user", "cairn-project-ws-domain");
    assert!(decode_intent_private_review_receipt_v1(self_review.as_bytes()).is_err());

    let incomplete = receipt.replace(",\"exposure-and-diagnostic-safety\"", "");
    assert!(decode_intent_private_review_receipt_v1(incomplete.as_bytes()).is_err());

    let wrong_domain = IntentArtifactIdentity::derive(b"synthetic public bundle")
        .expect("wrong semantic identity")
        .to_string();
    let wrong_binding = receipt.replace(&review_subject.to_string(), &wrong_domain);
    assert!(decode_intent_private_review_receipt_v1(wrong_binding.as_bytes()).is_err());
}
