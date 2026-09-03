//! Where a task's frozen source lives after the archive that carried it is gone.
//!
//! The archive is transport. What the system keeps is the per-path source bundle, and until now it
//! kept it only in the workflow process: unpacking produced an in-memory map, the bundle identity
//! was derived from it, and nothing wrote either one down. A restart therefore lost the source of
//! every task in flight while the event log still said those tasks had been submitted, which is a
//! record that cannot be acted on.
//!
//! This is deliberately the store rather than a directory tree. The source is content-addressed
//! material, so the store already knows how to keep it, verify it and deduplicate it, and a second
//! copy on disk would be a second answer to what the frozen source is.

use cairn_migration::{
    SirTaskArtifactBytes, SirTaskArtifactPath, SirTaskBundleArtifact, SirTaskBundleV1,
    SirTaskLimits, SirTaskWorkspace,
};
use cairn_protocol::{ContentId, TaskId};
use cairn_record::ContentStore;
use cairn_store_sqlite::SqliteContentStore;
use std::io::Cursor;

use crate::MigrationAppApiError;

/// Persists and recovers the frozen source of a task against one deployment's content store.
#[derive(Clone, Debug)]
pub struct TaskWorkspaceStoreV1 {
    server: cairn_server::ServerConfig,
}

impl TaskWorkspaceStoreV1 {
    /// Binds one deployment's content store as where frozen task source is kept.
    #[must_use]
    pub const fn new(server: cairn_server::ServerConfig) -> Self {
        Self { server }
    }

    /// Writes one task's frozen source so it outlives the process that froze it.
    ///
    /// Every artifact body is stored under the identity the bundle already names, and the bundle
    /// itself under its own. Storing the bodies first means a bundle that is present is always
    /// readable: the reverse order could leave a manifest naming material that is not there.
    ///
    /// # Errors
    ///
    /// Returns a store error, or an identity mismatch if a stored body did not hash to the
    /// identity its bundle claims.
    pub fn freeze(
        &self,
        task_id: TaskId,
        workspace: &SirTaskWorkspace,
    ) -> Result<ContentId<SirTaskBundleArtifact>, MigrationAppApiError> {
        let bundle = workspace.bundle().clone();
        let sources = workspace.materialized_sources();
        let bundle_bytes = cairn_codec::to_vec(&bundle).map_err(MigrationAppApiError::internal)?;
        let expected = workspace
            .bundle()
            .identity()
            .map_err(MigrationAppApiError::internal)?;
        let database = self.server.content_database();
        let directory = self.server.content_directory();
        tokio::task::block_in_place(move || {
            let mut content = SqliteContentStore::open(&database, &directory)
                .map_err(MigrationAppApiError::internal)?;
            for (path, source) in &sources {
                let stored = content
                    .put::<SirTaskArtifactBytes>(&mut Cursor::new(source.as_bytes().to_vec()))
                    .map_err(MigrationAppApiError::internal)?
                    .content_id;
                let declared = bundle
                    .artifacts()
                    .iter()
                    .find(|artifact| artifact.path() == path)
                    .map(cairn_migration::SirTaskArtifactV1::identity)
                    .ok_or(MigrationAppApiError::TaskWorkspaceIdentityMismatch)?;
                if stored != declared {
                    return Err(MigrationAppApiError::TaskWorkspaceIdentityMismatch);
                }
            }
            let stored = content
                .put::<SirTaskBundleArtifact>(&mut Cursor::new(bundle_bytes))
                .map_err(MigrationAppApiError::internal)?
                .content_id;
            if stored != expected {
                return Err(MigrationAppApiError::TaskWorkspaceIdentityMismatch);
            }
            tracing::info!(
                target: "cairn.migration.workspace",
                event = "task_source_frozen",
                task_id = %task_id,
                bundle_id = %stored,
                artifact_count = sources.len(),
                "frozen task source persisted to the deployment store"
            );
            Ok(stored)
        })
    }

    /// Rebuilds one task's frozen source from the store.
    ///
    /// Reconstruction is bound to the frozen bundle rather than re-derived from whatever came
    /// back, so a recovered workspace reports the bundle that was submitted or none at all.
    ///
    /// In practice the store's own integrity check fires first: it refuses a body whose bytes no
    /// longer match the identity they are filed under, so the mismatch this constructor guards
    /// against cannot currently be produced through this path. It is kept because it states what
    /// recovery means, not because it is the layer that catches tampering today.
    ///
    /// # Errors
    ///
    /// Returns a store error, or a task-material error if the stored material does not reconstruct
    /// the bundle it claims to be.
    pub fn recover(
        &self,
        bundle_id: ContentId<SirTaskBundleArtifact>,
        limits: SirTaskLimits,
    ) -> Result<SirTaskWorkspace, MigrationAppApiError> {
        let database = self.server.content_database();
        let directory = self.server.content_directory();
        tokio::task::block_in_place(move || {
            let content = SqliteContentStore::open(&database, &directory)
                .map_err(MigrationAppApiError::internal)?;
            let mut bundle_bytes = Vec::new();
            content
                .write_to(&bundle_id, &mut bundle_bytes)
                .map_err(MigrationAppApiError::internal)?;
            let bundle: SirTaskBundleV1 =
                cairn_codec::from_slice(&bundle_bytes).map_err(MigrationAppApiError::internal)?;
            let mut sources: Vec<(SirTaskArtifactPath, String)> = Vec::new();
            for artifact in bundle.artifacts() {
                let mut body = Vec::new();
                content
                    .write_to(&artifact.identity(), &mut body)
                    .map_err(MigrationAppApiError::internal)?;
                let source = String::from_utf8(body).map_err(MigrationAppApiError::internal)?;
                sources.push((artifact.path().clone(), source));
            }
            SirTaskWorkspace::from_materialized(bundle, sources, limits)
                .map_err(MigrationAppApiError::internal)
        })
    }
}
