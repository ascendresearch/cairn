//! A task's material has a lifecycle, so it lives where a lifecycle can end.
//!
//! Frozen source was briefly kept in the deployment's shared content store, which keeps bytes well
//! and keeps projects badly: material there is deduplicated, so removing a finished task would
//! have meant first working out who else was holding each of its files. These tests hold the
//! filing decision that avoids that — one directory per task, removed by removing the directory.

use std::error::Error;

use cairn_migration::{SirTaskArtifactPath, SirTaskLimits, SirTaskWorkspace};
use cairn_migration_app::TaskWorkspaceStoreV1;
use cairn_protocol::TaskId;

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
    let store = TaskWorkspaceStoreV1::new(directory.path().to_path_buf());
    let limits = SirTaskLimits::default();
    let workspace = SirTaskWorkspace::from_sources(sources()?, limits)?;
    let task_id = TaskId::new();

    store.freeze(task_id, &workspace)?;

    // Recovery reads the directory alone: nothing of the original workspace is carried over.
    let recovered = store.recover(task_id, limits)?;
    assert_eq!(recovered.bundle(), workspace.bundle());
    assert_eq!(
        recovered.source(&SirTaskArtifactPath::new("src/kernel.cu")?),
        Some("#include \"kernel.h\"\nvoid launch() {}\n")
    );
    Ok(())
}

// The reason this material is filed per task rather than in the shared store. Ending a task is
// removing one directory; nothing else in the deployment holds a piece of it, so there is no
// reference count to consult and no other task to disturb.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discarding_one_task_removes_its_material_and_leaves_its_neighbour_intact()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = TaskWorkspaceStoreV1::new(directory.path().to_path_buf());
    let limits = SirTaskLimits::default();
    let workspace = SirTaskWorkspace::from_sources(sources()?, limits)?;
    let first = TaskId::new();
    let second = TaskId::new();

    // Both tasks carry byte-identical source, which is exactly the case a deduplicating store
    // would have collapsed into one copy and then been unable to release for either task alone.
    store.freeze(first, &workspace)?;
    store.freeze(second, &workspace)?;

    store.discard(first)?;

    assert!(!store.task_directory(first).exists());
    assert!(store.recover(first, limits).is_err());
    assert_eq!(store.recover(second, limits)?.bundle(), workspace.bundle());
    // Discarding what is already gone is not a failure: the task's material is absent either way.
    store.discard(first)?;
    Ok(())
}

// A plain directory offers no integrity of its own, so the reconstruction against the frozen
// bundle is what stands between an edited file and a model reading it as the submitted source.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn source_edited_after_freezing_does_not_reconstruct()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let store = TaskWorkspaceStoreV1::new(directory.path().to_path_buf());
    let limits = SirTaskLimits::default();
    let workspace = SirTaskWorkspace::from_sources(sources()?, limits)?;
    let task_id = TaskId::new();
    store.freeze(task_id, &workspace)?;

    std::fs::write(
        store
            .task_directory(task_id)
            .join("source/include/kernel.h"),
        "#pragma once\nvoid launch(int);\n",
    )?;

    let Err(error) = store.recover(task_id, limits) else {
        panic!("source edited after freezing must not reconstruct");
    };
    assert!(
        format!("{error}").contains("frozen bundle"),
        "expected the edit to be caught against the frozen bundle, got: {error}"
    );
    Ok(())
}
