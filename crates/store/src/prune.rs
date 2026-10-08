use crate::{ScopedStore, StoreError, SyncGuard};
use ionoray_core::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};

/// Counts from one reference-safe scoped CAS cleanup.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PruneReport {
    /// Immutable CAS bodies removed.
    pub objects_removed: u64,
    /// Managed relative browse links removed.
    pub views_removed: u64,
}

impl ScopedStore {
    /// Removes bodies not in caller retention plus current accepted baselines.
    ///
    /// # Errors
    /// Returns an error for an unsafe guard, catalog row, or managed path.
    ///
    /// # Panics
    /// Panics only if the internal scoped object layout has no parent directory.
    pub async fn prune(
        &self,
        retained: &HashSet<Sha256Digest>,
        guard: &SyncGuard,
    ) -> Result<PruneReport, StoreError> {
        if guard.path != self.layout.temporary_root().join("locks/scope.lock") {
            return Err(StoreError::ScopeBusy(self.scope.as_str().to_owned()));
        }
        let mut keep = retained.clone();
        let connection = self.catalog.connect()?;
        let mut rows = connection
            .query("SELECT artifact_sha256, pending_previous_sha256, history_sha256 FROM accepted_source", ())
            .await?;
        while let Some(row) = rows.next().await? {
            for digest in [row.get::<Option<String>>(0)?, row.get(1)?, row.get(2)?]
                .into_iter()
                .flatten()
            {
                keep.insert(
                    digest
                        .parse()
                        .map_err(|_| StoreError::InvalidBrowsePath(self.layout.objects()))?,
                );
            }
        }
        let mut report = PruneReport::default();
        let mut views = connection
            .query(
                "SELECT browse_relative, artifact_sha256 FROM accepted_view",
                (),
            )
            .await?;
        let mut stale = Vec::new();
        while let Some(row) = views.next().await? {
            let relative = std::path::PathBuf::from(row.get::<String>(0)?);
            let digest: Sha256Digest = row
                .get::<String>(1)?
                .parse()
                .map_err(|_| StoreError::InvalidBrowsePath(relative.clone()))?;
            if !keep.contains(&digest) {
                stale.push(relative);
            }
        }
        drop(views);
        for relative in stale {
            if !relative.is_relative()
                || relative.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err(StoreError::InvalidBrowsePath(relative));
            }
            let path = self
                .layout
                .objects()
                .parent()
                .expect("scoped")
                .join(&relative);
            verify_browse_parent(self.layout.objects().parent().expect("scoped"), &path)?;
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    fs::remove_file(&path).map_err(|e| StoreError::io(&path, e))?;
                    report.views_removed += 1;
                }
                Ok(_) => return Err(StoreError::RefuseOverwriteView(path)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(StoreError::io(&path, e)),
            }
            connection
                .execute(
                    "DELETE FROM accepted_view WHERE browse_relative = ?",
                    [relative.to_string_lossy().as_ref()],
                )
                .await?;
        }
        let sha = self.layout.objects();
        for shard in fs::read_dir(&sha).map_err(|e| StoreError::io(&sha, e))? {
            let shard = shard.map_err(|e| StoreError::io(&sha, e))?.path();
            let metadata = fs::symlink_metadata(&shard).map_err(|e| StoreError::io(&shard, e))?;
            if metadata.file_type().is_symlink() {
                return Err(StoreError::InvalidBrowsePath(shard));
            }
            if !metadata.file_type().is_dir() {
                continue;
            }
            for entry in fs::read_dir(&shard).map_err(|e| StoreError::io(&shard, e))? {
                let path = entry.map_err(|e| StoreError::io(&shard, e))?.path();
                let Some(name) = path.file_name().and_then(|x| x.to_str()) else {
                    continue;
                };
                let Ok(digest) = name.parse::<Sha256Digest>() else {
                    continue;
                };
                if !keep.contains(&digest) {
                    fs::remove_file(&path).map_err(|e| StoreError::io(&path, e))?;
                    report.objects_removed += 1;
                }
            }
        }
        Ok(report)
    }
}

fn verify_browse_parent(root: &std::path::Path, path: &std::path::Path) -> Result<(), StoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| StoreError::InvalidBrowsePath(path.to_path_buf()))?;
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| StoreError::InvalidBrowsePath(path.to_path_buf()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        if fs::symlink_metadata(&current)
            .map_err(|e| StoreError::io(&current, e))?
            .file_type()
            .is_symlink()
        {
            return Err(StoreError::InvalidBrowsePath(current));
        }
    }
    Ok(())
}
