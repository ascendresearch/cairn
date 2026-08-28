use std::{fs, path::PathBuf, process::Command};

use cairn_testkit::fixtures::{
    DevelopmentSliceId, FixtureIdentity, GitCommitId, PublicFixturePath, SanitationCheckKind,
    decode_fixture_v1, decode_manifest_v1, decode_scan_profile_v1, scan_public_tree,
};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_root() -> PathBuf {
    repository_root().join("fixtures/regressions/v1")
}

fn profile_bytes() -> Vec<u8> {
    fs::read(fixture_root().join("sanitation-scan-profile.json")).expect("scan profile")
}

#[test]
fn public_manifest_recomputes_every_fixture_and_scan_is_clean() {
    let root = repository_root();
    let fixtures = fixture_root();
    let manifest_bytes = fs::read(fixtures.join("manifest.json")).expect("manifest");
    let manifest = decode_manifest_v1(&manifest_bytes).expect("strict manifest");
    assert_eq!(manifest.fixtures().len(), 7);
    manifest.validate_tree(&root).expect("fixture tree");

    let profile = profile_bytes();
    assert_eq!(
        decode_scan_profile_v1(&profile)
            .expect("profile")
            .checks()
            .len(),
        7
    );
    let report = scan_public_tree(&root, &fixtures, &profile).expect("public scan");
    assert!(report.is_clean(), "findings: {:?}", report.findings());
    assert_eq!(report.scanned_paths().len(), 10);

    assert!(!fixtures.join("workflows/st1-identity-graph.json").exists());
    assert!(!fixtures.join("st1-identity-graph-plan.md").exists());
}

#[test]
fn fixture_decode_is_strict_and_identity_changes_with_bytes() {
    let path = fixture_root().join("record/model-input-audit.json");
    let bytes = fs::read(path).expect("fixture");
    let fixture = decode_fixture_v1(&bytes).expect("strict fixture");
    assert_eq!(fixture.cases().len(), 4);
    let identity = FixtureIdentity::derive(&bytes).expect("identity");

    let mut noncanonical = bytes.clone();
    noncanonical.push(b'\n');
    assert!(decode_fixture_v1(&noncanonical).is_err());
    assert_ne!(
        identity,
        FixtureIdentity::derive(&noncanonical).expect("changed identity")
    );

    let non_v1 = bytes
        .windows(b"\"schema_version\":1".len())
        .position(|window| window == b"\"schema_version\":1")
        .expect("schema marker");
    let mut changed = bytes;
    changed[non_v1 + b"\"schema_version\":".len()] = b'2';
    assert!(decode_fixture_v1(&changed).is_err());

    let contradictory = String::from_utf8(noncanonical[..noncanonical.len() - 1].to_vec())
        .expect("fixture text")
        .replace("\"expected\":\"complete\"", "\"expected\":\"blocked\"");
    assert!(decode_fixture_v1(contradictory.as_bytes()).is_err());
}

#[test]
fn manifest_rejects_changed_fixture_identity_and_paths_rerun_validation() {
    let bytes = fs::read(fixture_root().join("manifest.json")).expect("manifest");
    let needle = b"f7bab94a29ff5f5387e7eac0699afe957f5ee84bc1b965ac7229fc66a9f2e3da";
    let offset = bytes
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("fixture identity");
    let mut tampered = bytes;
    tampered[offset] = b'0';
    assert!(decode_manifest_v1(&tampered).is_ok());
    assert!(
        decode_manifest_v1(&tampered)
            .expect("shape remains valid")
            .validate_tree(&repository_root())
            .is_err()
    );

    assert!(PublicFixturePath::new("/absolute/fixture.json").is_err());
    assert!(PublicFixturePath::new("fixtures/regressions/v1/../escape.json").is_err());
    assert!(PublicFixturePath::new("fixtures/regressions/v1/restricted/case.json").is_err());
    assert!(GitCommitId::new("688b637").is_err());
    assert!(DevelopmentSliceId::new("dev-three").is_err());

    let bytes = fs::read(fixture_root().join("manifest.json")).expect("manifest");
    let wrong_source = String::from_utf8(bytes).expect("manifest text").replacen(
        "253df4118ab9b8e9e7d2d5a70f8f92147735f1dbf936e850111a92a4ba4615a8",
        "3287d46e1ef34faf4d45e843932ca2d29df30ee2a49f65acd8eff235c6a5b229",
        1,
    );
    assert!(decode_manifest_v1(wrong_source.as_bytes()).is_err());
}

#[test]
fn seeded_sanitation_controls_fail_closed_without_exposing_snippets() {
    let profile = profile_bytes();
    let controls = [
        (
            "credential.json",
            b"-----BEGIN PRIVATE KEY-----".as_slice(),
            SanitationCheckKind::CredentialMaterial,
        ),
        (
            "provider.json",
            br#"{"raw_response":"canary"}"#,
            SanitationCheckKind::ProviderBody,
        ),
        (
            "host-path.json",
            br#"{"path":"/home/example/input"}"#,
            SanitationCheckKind::AbsoluteHostPath,
        ),
        (
            "state.sqlite3",
            b"canary".as_slice(),
            SanitationCheckKind::DatabaseState,
        ),
        ("binary.json", &[0xff, 0xfe], SanitationCheckKind::NonUtf8),
    ];
    for (name, bytes, expected) in controls {
        let temporary = tempfile::tempdir().expect("temporary repository");
        let public = temporary.path().join("fixtures/regressions/v1");
        fs::create_dir_all(&public).expect("public fixture root");
        fs::write(public.join(name), bytes).expect("seed canary");
        let report = scan_public_tree(temporary.path(), &public, &profile).expect("scan control");
        assert_eq!(report.findings().len(), 1);
        assert_eq!(report.findings()[0].check(), expected);
    }

    for forbidden in ["secrets/case.json", "restricted/case.json"] {
        let temporary = tempfile::tempdir().expect("temporary repository");
        let public = temporary.path().join("fixtures/regressions/v1");
        let path = public.join(forbidden);
        fs::create_dir_all(path.parent().expect("parent")).expect("forbidden directory");
        fs::write(path, b"canary").expect("seed path canary");
        assert!(scan_public_tree(temporary.path(), &public, &profile).is_err());
    }
}

#[test]
fn private_runtime_tree_is_not_git_tracked_when_repository_metadata_is_available() {
    let root = repository_root();
    if !root.join(".git").exists() {
        return;
    }
    let output = Command::new("git")
        .args(["ls-files", ".cairn"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());

    let manifest =
        decode_manifest_v1(&fs::read(fixture_root().join("manifest.json")).expect("manifest"))
            .expect("strict manifest");
    for source in manifest.source_references() {
        let body = source.body();
        let object = format!("{}:{}", body.commit().as_str(), body.path().as_str());
        let status = Command::new("git")
            .args(["cat-file", "-e", &object])
            .current_dir(&root)
            .status()
            .expect("git cat-file");
        assert!(status.success(), "missing historical source {object}");
    }
}
