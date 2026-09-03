//! Where a task's frozen source lives after the archive that carried it is gone.
//!
//! It lives in that task's own directory, and nowhere else. The alternative was the deployment's
//! content store, which is a better place to keep bytes and a worse place to keep a project:
//! material there is shared and deduplicated, so a project that ends cannot be removed without
//! first working out who else is holding each of its files. That is a reference count, and a
//! reference count is machinery this product would be building only to undo a filing decision.
//!
//! Here the whole of a task's material is one directory. Removing it is removing the directory,
//! and nothing else in the deployment is holding a piece of it.
//!
//! The frozen source is never a build input. A build assembles from the Controller's recipe and
//! the candidate's own submitted files, so the caution in `ARCHITECTURE.md` 10.6 about building
//! out of a long-lived mutable directory does not reach this material: it is read-only reference
//! material for reasoning, and its integrity is checked on the way back in.

use std::path::PathBuf;

use cairn_migration::{SirTaskArtifactPath, SirTaskBundleV1, SirTaskLimits, SirTaskWorkspace};
use cairn_protocol::TaskId;

use crate::MigrationAppApiError;

const BUNDLE: &str = "task-bundle.json";
const SOURCE: &str = "source";

/// Persists and recovers the frozen source of one task inside that task's own directory.
#[derive(Clone, Debug)]
pub struct TaskWorkspaceStoreV1 {
    root: PathBuf,
}

impl TaskWorkspaceStoreV1 {
    /// Binds the deployment's workspace tree as where task material is kept.
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the directory holding everything this task owns.
    #[must_use]
    pub fn task_directory(&self, task_id: TaskId) -> PathBuf {
        self.root.join(task_id.to_string())
    }

    /// Writes one task's frozen source so it outlives the process that froze it.
    ///
    /// The bundle is written last. It is the manifest recovery starts from, so a directory that
    /// has one always has the material it names; the reverse order could leave a manifest
    /// pointing at files that were never written.
    ///
    /// # Errors
    ///
    /// Returns an I/O or encoding error.
    pub fn freeze(
        &self,
        task_id: TaskId,
        workspace: &SirTaskWorkspace,
    ) -> Result<(), MigrationAppApiError> {
        let directory = self.task_directory(task_id);
        let bundle =
            cairn_codec::to_vec(workspace.bundle()).map_err(MigrationAppApiError::internal)?;
        let sources = workspace.materialized_sources();
        tokio::task::block_in_place(move || {
            for (path, source) in &sources {
                let file = source_path(&directory, path)?;
                if let Some(parent) = file.parent() {
                    std::fs::create_dir_all(parent).map_err(MigrationAppApiError::io)?;
                }
                std::fs::write(&file, source).map_err(MigrationAppApiError::io)?;
            }
            std::fs::create_dir_all(&directory).map_err(MigrationAppApiError::io)?;
            std::fs::write(directory.join(BUNDLE), bundle).map_err(MigrationAppApiError::io)?;
            tracing::info!(
                target: "cairn.migration.workspace",
                event = "task_source_frozen",
                task_id = %task_id,
                artifact_count = sources.len(),
                "frozen task source persisted to the task's own directory"
            );
            Ok(())
        })
    }

    /// Rebuilds one task's frozen source from its directory.
    ///
    /// Reconstruction is bound to the frozen bundle, so what comes back is the source that was
    /// submitted or nothing. This is the check that matters here: a plain directory offers no
    /// integrity of its own, and a file edited in place would otherwise reach a model as though
    /// it were what the caller sent.
    ///
    /// # Errors
    ///
    /// Returns an I/O, encoding, or task-material error when the directory does not reconstruct
    /// the bundle it carries.
    pub fn recover(
        &self,
        task_id: TaskId,
        limits: SirTaskLimits,
    ) -> Result<SirTaskWorkspace, MigrationAppApiError> {
        let directory = self.task_directory(task_id);
        tokio::task::block_in_place(move || {
            let bundle = std::fs::read(directory.join(BUNDLE)).map_err(MigrationAppApiError::io)?;
            let bundle: SirTaskBundleV1 =
                cairn_codec::from_slice(&bundle).map_err(MigrationAppApiError::internal)?;
            let mut sources = Vec::new();
            for artifact in bundle.artifacts() {
                let file = source_path(&directory, artifact.path())?;
                let source = std::fs::read_to_string(&file).map_err(MigrationAppApiError::io)?;
                sources.push((artifact.path().clone(), source));
            }
            SirTaskWorkspace::from_materialized(bundle, sources, limits)
                .map_err(MigrationAppApiError::internal)
        })
    }

    /// Removes everything this task owns.
    ///
    /// It is one directory, so this is one removal. Nothing else in the deployment holds a piece
    /// of this task's material, which is the whole reason the material is filed this way.
    ///
    /// # Errors
    ///
    /// Returns an I/O error. A task with no directory is already removed and is not an error.
    pub fn discard(&self, task_id: TaskId) -> Result<(), MigrationAppApiError> {
        let directory = self.task_directory(task_id);
        tokio::task::block_in_place(move || match std::fs::remove_dir_all(&directory) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(MigrationAppApiError::io(error)),
        })
    }
}

/// Resolves one task-local artifact path inside a task directory.
///
/// The path came from an archive, so it is checked rather than trusted: a component that climbs
/// out of the task directory would let a submission write anywhere the process can reach.
fn source_path(
    directory: &std::path::Path,
    path: &SirTaskArtifactPath,
) -> Result<PathBuf, MigrationAppApiError> {
    let mut resolved = directory.join(SOURCE);
    for component in std::path::Path::new(path.as_str()).components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            _ => return Err(MigrationAppApiError::TaskWorkspacePathEscape),
        }
    }
    Ok(resolved)
}
