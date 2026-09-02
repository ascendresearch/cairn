use std::{error::Error, fs, process::Command};

/// Intake is an administrative act, so its gate has to be reachable the way an administrator
/// reaches it. These run the real binary against a real workspace tree rather than calling the
/// validator directly, because a gate nobody can invoke is not a gate.
#[test]
fn project_intake_admits_a_complete_definition_and_refuses_an_ambiguous_one()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("deployment");
    fs::create_dir_all(home.join("store"))?;
    fs::create_dir_all(home.join("workspaces/reduce-sum-f32"))?;
    let config = directory.path().join("controller.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&serde_json::json!({
            "authority_poll_interval_ms": 25,
            "diagnostic_byte_limit": null,
            "enrollment_service": null,
            "handshake_timeout_ms": null,
            "idle_timeout_ms": null,
            "store_root": home.join("store"),
            "workspaces_root": home.join("workspaces"),
            "listen": "127.0.0.1:7443",
            "outbox_poll_interval_ms": null,
            "protocol_version": 1,
            "schema_version": 1,
            "scheduler": null,
            "tls": {
                "certificate": home.join("secrets/unused-controller.pem"),
                "client_ca": home.join("secrets/unused-ca.pem"),
                "private_key": home.join("secrets/unused-controller-key.pem")
            },
            "transport": { "message_byte_limit": null }
        }))?,
    )?;

    let definition = home.join("workspaces/reduce-sum-f32/project.json");
    let mut document = serde_json::json!({
        "schema_version": 1,
        "project": "reduce-sum-f32",
        "source": {
            "upstream": {
                "kind": "git",
                "repository": "https://example.test/kernels.git",
                "commit": "0123456789abcdef0123456789abcdef01234567"
            }
        },
        "provided": ["bin/run", "CMakeLists.txt"],
        "authored_by_agent": ["source/kernel.cpp"]
    });
    fs::write(&definition, serde_json::to_vec_pretty(&document)?)?;

    let admitted = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .args(["project", "validate"])
        .arg(&config)
        .arg("reduce-sum-f32")
        .output()?;
    assert!(
        admitted.status.success(),
        "{}",
        String::from_utf8_lossy(&admitted.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&admitted.stdout)?;
    assert_eq!(report["project"], "reduce-sum-f32");
    assert_eq!(report["provided"], 2);
    assert_eq!(report["authored_by_agent"], 1);
    assert_eq!(
        report["workspace"],
        home.join("workspaces/reduce-sum-f32")
            .to_string_lossy()
            .as_ref()
    );

    // The same definition with one file claimed by both sets must not enter intake, because a
    // build failure under it could be the candidate's fault or the scaffolding's.
    document["provided"] = serde_json::json!(["bin/run", "source/kernel.cpp"]);
    fs::write(&definition, serde_json::to_vec_pretty(&document)?)?;
    let refused = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .args(["project", "validate"])
        .arg(&config)
        .arg("reduce-sum-f32")
        .output()?;
    assert!(!refused.status.success());
    let diagnostic = String::from_utf8_lossy(&refused.stderr);
    assert!(
        diagnostic.contains("declared both provided and authored by the agent"),
        "the refusal must name the ambiguity: {diagnostic}"
    );
    Ok(())
}
