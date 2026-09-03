//! Frozen task source has to outlive the process that froze it.
//!
//! Before this existed, unpacking a submission produced an in-memory map and nothing wrote it
//! down. The event log said a task had been submitted while the only copy of what that task was
//! about lived in one process, so a restart left a record nobody could act on.

use std::error::Error;

use cairn_migration::{SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace};
use cairn_migration_app::TaskWorkspaceStoreV1;
use cairn_protocol::TaskId;
use cairn_server::ServerConfig;

fn server(store_root: &std::path::Path) -> Result<ServerConfig, Box<dyn Error + Send + Sync>> {
    Ok(serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "listen": "127.0.0.1:0",
        "tls": {
            "certificate": "secrets/controller.pem",
            "private_key": "secrets/controller-key.pem",
            "client_ca": "secrets/ca.pem"
        },
        "enrollment_service": null,
        "protocol_version": 1,
        "scheduler": null,
        "handshake_timeout_ms": 10_000,
        "idle_timeout_ms": 120_000,
        "outbox_poll_interval_ms": 100,
        "authority_poll_interval_ms": 1_000,
        "diagnostic_byte_limit": 1_024,
        "store_root": store_root,
    }))?)
}

fn sources() -> Result<Vec<(SirTaskArtifactPath, String)>, Box<dyn Error + Send + Sync>> {
    Ok(vec![
        (
            SirTaskArtifactPath::new("include/kernel.h")?,
            "#pragma once\nvoid launch();\n".to_owned(),
        ),
        (
            SirTaskArtifactPath::new("src/kernel.cu")?,
            "#include \"kernel.h\"\nvoid launch() {}\n".to_owned(),
        ),
    ])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn frozen_source_survives_the_process_that_froze_it()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = TaskWorkspaceStoreV1::new(server(directory.path())?);
    let limits = SirTaskLimits::default();
    let workspace = SirTaskWorkspace::from_sources(sources()?, limits)?;

    let bundle_id = store.freeze(TaskId::new(), &workspace)?;

    // Recovery must reconstruct against the frozen bundle rather than re-deriving a new one from
    // whatever bytes came back. Re-deriving would make any coherent material look like the
    // submitted source, so the bundle a recovered workspace reports has to be the frozen one.
    let recovered_bundle = store.recover(bundle_id, limits)?;
    assert_eq!(
        recovered_bundle
            .bundle()
            .identity()
            .expect("recovered bundle identity"),
        bundle_id
    );

    // Recovery goes through the store alone: no part of the original workspace is carried over.
    let recovered = store.recover(bundle_id, limits)?;
    assert_eq!(recovered.bundle(), workspace.bundle());
    assert_eq!(
        recovered.source(&SirTaskArtifactPath::new("src/kernel.cu")?),
        Some("#include \"kernel.h\"\nvoid launch() {}\n")
    );
    // Freezing the same source twice is the same bundle: the store deduplicates by content, so a
    // resubmission of identical material does not become a second frozen source.
    assert_eq!(store.freeze(TaskId::new(), &workspace)?, bundle_id);
    Ok(())
}

// Content addressing is only a guarantee if something checks it. A body rewritten on disk is
// caught by the store's own integrity check, before this module's reconstruction is reached: the
// point of the test is that the tampered bytes never come back, not which layer says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_that_no_longer_matches_its_bundle_is_refused()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = TaskWorkspaceStoreV1::new(server(directory.path())?);
    let limits = SirTaskLimits::default();
    let workspace = SirTaskWorkspace::from_sources(sources()?, limits)?;
    let bundle_id = store.freeze(TaskId::new(), &workspace)?;

    // Rewrite one artifact's body in place, leaving the bundle naming the identity it no longer
    // has. This is what a corrupted or tampered store looks like from the reader's side.
    for entry in walk(&directory.path().join("content")) {
        if std::fs::read(&entry)? == b"#pragma once\nvoid launch();\n" {
            std::fs::write(&entry, b"#pragma once\nvoid launch(int);\n")?;
        }
    }

    let Err(error) = store.recover(bundle_id, limits) else {
        panic!("source that no longer matches its bundle must not reconstruct");
    };
    assert!(
        format!("{error}").contains("content integrity failure"),
        "expected the store to refuse the rewritten body, got: {error}"
    );
    Ok(())
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}
