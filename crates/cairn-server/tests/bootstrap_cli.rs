use std::{error::Error, fs, net::TcpListener, process::Command, time::Duration};

/// The only question worth asking about a layout is whether a deployment built from it starts.
/// Asserting that the directories exist would be checking what the same code just created, so this
/// runs the real command and then runs the real server against what it produced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bootstrapped_deployment_starts() -> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("deployment");
    let control = free_address()?;
    let enrollment = free_address()?;

    let created = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .arg("bootstrap")
        .arg(&root)
        .args(["localhost", &control, &enrollment])
        .output()?;
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for (tree, mode) in [
            ("secrets", 0o700),
            ("restricted", 0o700),
            ("store", 0o755),
            ("workspaces", 0o755),
        ] {
            assert_eq!(
                fs::metadata(root.join(tree))?.permissions().mode() & 0o777,
                mode,
                "{tree} must carry the mode its material class calls for"
            );
        }
        assert_eq!(
            fs::metadata(root.join("secrets/ca-key.pem"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    let config = cairn_server::load_server_config(&root.join("config/controller.json"))?;
    let server = tokio::spawn(cairn_server::run(config));
    tokio::time::sleep(Duration::from_millis(400)).await;
    if server.is_finished() {
        return Err(format!(
            "a bootstrapped deployment did not start: {:?}",
            server.await
        )
        .into());
    }
    // The store is where a running controller writes, so its presence is evidence the process got
    // past configuration and opened what it was pointed at.
    assert!(root.join("store/events.sqlite3").is_file());
    server.abort();
    Ok(())
}

/// Bootstrap never merges into an existing deployment, because every answer to "what about the
/// material already here" is worse than refusing.
#[test]
fn bootstrap_refuses_a_directory_that_is_not_empty() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    fs::write(directory.path().join("occupied"), b"x")?;
    let refused = Command::new(env!("CARGO_BIN_EXE_cairn-server"))
        .arg("bootstrap")
        .arg(directory.path())
        .args(["localhost", "127.0.0.1:1", "127.0.0.1:2"])
        .output()?;
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("is not empty"),
        "the refusal must say why"
    );
    Ok(())
}

fn free_address() -> Result<String, Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.to_string())
}
