use std::{fs, path::Path, time::Instant};

use serde::{Deserialize, Serialize};
use tracing::Instrument;
use turso::Database;

use crate::{
    ArtifactOrigin, CatalogArtifact, DatabaseKind, DatabaseStatus, DownloadError, DownloadManager,
    MaintenanceRun, MaintenanceSummary, StoreError, StoreHome, StoreLayout,
    database::open_database,
    maintenance,
    objects::{artifacts_for_year, origins},
    scope::{ScopedStore, StoreScope},
};

/// Open Turso databases rooted in one initialized `IonoRay` home.
pub struct Store {
    home: StoreHome,
    layout: StoreLayout,
    object_catalog: Database,
    status: StoreStatus,
}

/// Root home handle for applications that use only isolated dataset scopes.
///
/// Opening this handle never creates the legacy global object catalog or CAS.
pub struct StoreRoot {
    home: StoreHome,
    layout: StoreLayout,
}

impl StoreRoot {
    /// Resolved runtime home.
    pub fn home(&self) -> &StoreHome {
        &self.home
    }

    /// Root path layout. Scoped operations must be opened through [`Self::scoped`].
    pub fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    /// Opens one isolated dataset scope.
    ///
    /// # Errors
    /// Returns an error when the scope layout or catalog cannot be opened.
    pub async fn scoped(&self, scope: StoreScope) -> Result<ScopedStore, StoreError> {
        open_scoped_layout(self.layout.scoped(scope.as_str()), scope).await
    }
}

impl Store {
    /// Opens a root handle without initializing the legacy global object store.
    ///
    /// # Errors
    /// Returns an error when the home or shared index directory cannot be initialized.
    pub fn open_root(explicit_home: Option<&Path>) -> Result<StoreRoot, StoreError> {
        let home = StoreHome::discover(explicit_home)?;
        let layout = StoreLayout::new(&home);
        fs::create_dir_all(home.as_path())
            .map_err(|error| StoreError::io(home.as_path(), error))?;
        fs::create_dir_all(home.as_path().join("indices"))
            .map_err(|error| StoreError::io(home.as_path().join("indices"), error))?;
        Ok(StoreRoot { home, layout })
    }

    /// Opens just one scoped dataset store without creating the legacy global catalog.
    ///
    /// New dataset integrations should use this constructor. [`Store::open`]
    /// remains available for callers that still use the legacy global API.
    ///
    /// # Errors
    /// Returns an error when the scoped catalog cannot be opened.
    pub async fn open_scoped(
        explicit_home: Option<&Path>,
        scope: StoreScope,
    ) -> Result<ScopedStore, StoreError> {
        Self::open_root(explicit_home)?.scoped(scope).await
    }

    /// Discovers, initializes, and opens a store.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when home resolution, filesystem initialization,
    /// schema inspection, or Turso creation fails.
    pub async fn open(explicit_home: Option<&Path>) -> Result<Self, StoreError> {
        let span = tracing::debug_span!(
            "store.open",
            home = ?explicit_home,
            elapsed_us = tracing::field::Empty,
        );
        let started = Instant::now();
        let result = async {
            let home = StoreHome::discover(explicit_home)?;
            let layout = StoreLayout::new(&home);
            layout.initialize()?;
            let (object_catalog, object_catalog_status) =
                open_database(&layout.object_catalog(), DatabaseKind::ObjectCatalog).await?;
            maintenance::interrupt_stale(&object_catalog).await?;

            Ok(Self {
                home,
                layout,
                object_catalog,
                status: StoreStatus {
                    object_catalog: object_catalog_status,
                },
            })
        };
        let result = result.instrument(span.clone()).await;
        span.record(
            "elapsed_us",
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        if let Err(error) = &result {
            tracing::warn!(parent: &span, error = %error, "failed to open store");
        }
        result
    }

    /// Explicitly initializes the local layout and current object catalog.
    ///
    /// All dependent operations call the same idempotent initialization path.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when layout or Turso initialization fails.
    #[tracing::instrument(
        name = "store.init",
        level = "info",
        skip_all,
        fields(home = ?explicit_home),
        err(level = "warn")
    )]
    pub async fn init(explicit_home: Option<&Path>) -> Result<StoreInitReport, StoreError> {
        let store = Self::open(explicit_home).await?;
        Ok(StoreInitReport {
            home: store.home.as_path().to_path_buf(),
            object_catalog: store.status.object_catalog.clone(),
        })
    }

    /// Resolved home directory.
    pub const fn home(&self) -> &StoreHome {
        &self.home
    }

    /// Canonical path layout.
    pub const fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    /// Current database schema identities.
    pub const fn status(&self) -> &StoreStatus {
        &self.status
    }

    /// Creates a conditional HTTP and CAS download manager.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError::Http`] if the reusable HTTP client cannot be built.
    pub fn downloads(&self) -> Result<DownloadManager<'_>, DownloadError> {
        DownloadManager::new(&self.object_catalog, &self.layout)
    }

    /// Opens an isolated dataset catalog, CAS, temporary directory, and locks.
    ///
    /// Candidate downloads from this view only use explicitly accepted objects
    /// as conditional HTTP baselines.
    ///
    /// # Errors
    /// Returns an error when the scoped catalog cannot be opened.
    pub async fn scoped(&self, scope: StoreScope) -> Result<ScopedStore, StoreError> {
        open_scoped_layout(self.layout.scoped(scope.as_str()), scope).await
    }

    /// Returns every CAS artifact mapped to one dataset-year partition.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] when catalog rows cannot be read safely.
    pub async fn artifacts_for_year(
        &self,
        provider: &str,
        dataset: &str,
        year: u16,
    ) -> Result<Vec<CatalogArtifact>, DownloadError> {
        artifacts_for_year(&self.object_catalog, &self.layout, provider, dataset, year).await
    }

    /// Resolves every upstream URL and filename associated with one CAS digest.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] when origin rows cannot be read.
    pub async fn artifact_origins(
        &self,
        digest: ionoray_core::Sha256Digest,
    ) -> Result<Vec<ArtifactOrigin>, DownloadError> {
        origins(&self.object_catalog, digest).await
    }

    /// Begins an auditable data-maintenance operation.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the audit record cannot be inserted.
    pub async fn begin_maintenance(
        &self,
        operation: &str,
        policy: &str,
        year: Option<u16>,
    ) -> Result<MaintenanceRun, StoreError> {
        maintenance::begin(&self.object_catalog, operation, policy, year).await
    }

    /// Marks a maintenance operation successful with aggregate counters.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the audit record cannot be updated.
    pub async fn finish_maintenance(
        &self,
        run: &MaintenanceRun,
        summary: MaintenanceSummary,
    ) -> Result<(), StoreError> {
        maintenance::finish(&self.object_catalog, run, summary).await
    }

    /// Marks a maintenance operation failed without hiding its audit record.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the failure record cannot be persisted.
    pub async fn fail_maintenance(
        &self,
        run: &MaintenanceRun,
        kind: &str,
        message: &str,
    ) -> Result<(), StoreError> {
        maintenance::fail(&self.object_catalog, run, kind, message).await
    }
}

async fn open_scoped_layout(
    layout: StoreLayout,
    scope: StoreScope,
) -> Result<ScopedStore, StoreError> {
    layout.initialize()?;
    let (catalog, _) =
        open_database(&layout.object_catalog(), DatabaseKind::ScopedObjectCatalog).await?;
    Ok(ScopedStore::new(scope, layout, catalog))
}

/// Database schema state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoreStatus {
    /// Object catalog schema state.
    pub object_catalog: DatabaseStatus,
}

/// Result of an explicit idempotent initialization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoreInitReport {
    /// Resolved data home.
    pub home: std::path::PathBuf,
    /// Current object catalog schema state.
    pub object_catalog: DatabaseStatus,
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use url::Url;

    use super::*;
    use crate::DownloadRequest;

    #[tokio::test]
    async fn opens_and_reuses_current_store_schema() {
        let temporary = TempDir::new().unwrap();

        let first = Store::open(Some(temporary.path())).await.unwrap();
        assert!(first.status().object_catalog.rebuilt);
        drop(first);

        let second = Store::open(Some(temporary.path())).await.unwrap();
        assert!(!second.status().object_catalog.rebuilt);
        assert!(second.layout().objects().is_dir());
    }

    #[tokio::test]
    async fn explicit_init_is_idempotent() {
        let temporary = TempDir::new().unwrap();
        let first = Store::init(Some(temporary.path())).await.unwrap();
        let second = Store::init(Some(temporary.path())).await.unwrap();
        assert!(first.object_catalog.rebuilt);
        assert!(!second.object_catalog.rebuilt);
    }

    #[tokio::test]
    async fn scoped_acceptance_creates_an_isolated_relative_browse_view() {
        let temporary = TempDir::new().unwrap();
        let root = Store::open_root(Some(temporary.path())).unwrap();
        let scoped = root.scoped(StoreScope::new("dst").unwrap()).await.unwrap();
        let request = DownloadRequest::new(
            "kyoto.dst.202001",
            Url::parse("https://example.test/dst/202001/dst2001.for?edition=final").unwrap(),
            "dst2001.for",
        )
        .unwrap();
        let candidate = scoped
            .downloads()
            .unwrap()
            .ingest_bundled(&request, b"accepted body", 0)
            .await
            .unwrap();
        let accepted = scoped.accept(&request, &candidate.artifact).await.unwrap();

        let link_metadata = std::fs::symlink_metadata(&accepted.browse_path).unwrap();
        assert!(link_metadata.file_type().is_symlink());
        let target = std::fs::read_link(&accepted.browse_path).unwrap();
        assert!(target.is_relative());
        assert_eq!(
            accepted.browse_path.canonicalize().unwrap(),
            candidate.artifact.path.canonicalize().unwrap()
        );
        let equivalent = scoped
            .downloads()
            .unwrap()
            .ingest_bundled(&request, b"accepted body", 1_000)
            .await
            .unwrap();
        let accepted_again = scoped.accept(&request, &equivalent.artifact).await.unwrap();
        assert_eq!(accepted.browse_path, accepted_again.browse_path);
        assert_eq!(
            accepted.accepted_at_utc_ms,
            accepted_again.accepted_at_utc_ms
        );
        assert_eq!(scoped.repair_views().await.unwrap(), 1);
        let mut permissions = std::fs::metadata(&candidate.artifact.path)
            .unwrap()
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        std::fs::set_permissions(&candidate.artifact.path, permissions).unwrap();
        std::fs::write(&candidate.artifact.path, b"corrupt").unwrap();
        assert!(scoped.repair_views().await.is_err());
        std::fs::remove_file(&candidate.artifact.path).unwrap();
        assert!(scoped.repair_views().await.is_err());
        assert!(scoped.layout().object_catalog().is_file());
        assert!(!temporary.path().join("objects/catalog.db").exists());
        assert!(!temporary.path().join("objects/sha256").exists());
    }

    #[tokio::test]
    async fn pending_raw_body_survives_until_non_revision_finalize() {
        let temporary = TempDir::new().unwrap();
        let scoped = Store::open_scoped(Some(temporary.path()), StoreScope::new("dst").unwrap())
            .await
            .unwrap();
        let request = DownloadRequest::new(
            "dst.source",
            Url::parse("https://example.test/dst/file").unwrap(),
            "file",
        )
        .unwrap();
        let first = scoped
            .downloads()
            .unwrap()
            .ingest_bundled(&request, b"old", 0)
            .await
            .unwrap();
        scoped.accept(&request, &first.artifact).await.unwrap();
        let second = scoped
            .downloads()
            .unwrap()
            .ingest_bundled(&request, b"new", 0)
            .await
            .unwrap();
        scoped.accept(&request, &second.artifact).await.unwrap();
        let guard = scoped.try_acquire_sync_lock().unwrap();
        scoped
            .prune(&std::collections::HashSet::new(), &guard)
            .await
            .unwrap();
        assert!(first.artifact.path.exists());
        scoped
            .finalize_acceptance(request.source_id(), false)
            .await
            .unwrap();
        scoped
            .prune(&std::collections::HashSet::new(), &guard)
            .await
            .unwrap();
        assert!(!first.artifact.path.exists());
    }
}
