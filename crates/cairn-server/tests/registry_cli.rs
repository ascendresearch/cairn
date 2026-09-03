use std::{error::Error, fs, process::Command};

use cairn_protocol::WorkerId;
use cairn_server::{WorkerRegistryAudit, WorkerRegistryInspection};

#[test]
fn registry_query_cli_emits_strict_json_and_missing_entries_fail() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("deployment");
    fs::create_dir_all(home.join("store"))?;
    let config = directory.path().join("controller.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&serde_json::json!({
            "authority_poll_interval_ms": 25,
            "diagnostic_byte_limit": null,
            "enrollment_service": null,
            "handshake_timeout_ms": null,
            "idle_timeout_ms": null,
            "listen": "127.0.0.1:7443",
            "outbox_poll_interval_ms": null,
            "protocol_version": 1,
            "schema_version": 1,
            "scheduler": null,
            "store_root": home.join("store"),
            "tls": {
                "certificate": home.join("secrets/unused-controller.pem"),
                "client_ca": home.join("secrets/unused-ca.pem"),
                "private_key": home.join("secrets/unused-controller-key.pem")
            },
            "transport": { "message_byte_limit": null }
        }))?,
    )?;

    let list = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .args(["registry", "list"])
        .arg(&config)
        .output()?;
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let inspection: WorkerRegistryInspection = serde_json::from_slice(&list.stdout)?;
    assert!(inspection.workers().is_empty());
    assert!(inspection.credentials().is_empty());
    assert_eq!(inspection.event_count(), 0);

    let audit = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .args(["registry", "audit"])
        .arg(&config)
        .output()?;
    assert!(
        audit.status.success(),
        "{}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit: WorkerRegistryAudit = serde_json::from_slice(&audit.stdout)?;
    assert_eq!(audit.worker_count(), 0);
    assert_eq!(audit.credential_count(), 0);

    let missing = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .args(["registry", "show-worker"])
        .arg(&config)
        .arg(WorkerId::new().to_string())
        .output()?;
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("registry entry not found"));
    Ok(())
}
