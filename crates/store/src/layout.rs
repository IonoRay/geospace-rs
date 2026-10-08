use std::{
    fs,
    path::{Path, PathBuf},
};

use ionoray_core::Sha256Digest;

use crate::StoreHome;

/// Canonical paths within one `IonoRay` home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreLayout {
    root: PathBuf,
    scope: Option<String>,
}

impl StoreLayout {
    /// Creates a path view without changing the filesystem.
    pub fn new(home: &StoreHome) -> Self {
        Self {
            root: home.as_path().to_path_buf(),
            scope: None,
        }
    }

    /// Creates an isolated object/catalog view for one dataset scope.
    pub(crate) fn scoped(&self, scope: &str) -> Self {
        Self {
            root: self.root.clone(),
            scope: Some(scope.to_owned()),
        }
    }

    /// Store root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Global CAS object and upstream-origin catalog.
    pub fn object_catalog(&self) -> PathBuf {
        match &self.scope {
            Some(scope) => self.root.join("objects").join(scope).join("catalog.db"),
            None => self.root.join("objects/catalog.db"),
        }
    }

    /// One source-specific yearly index database.
    pub fn index_database(&self, provider: &str, dataset: &str, year: u16) -> PathBuf {
        self.root
            .join("indices")
            .join(provider)
            .join(dataset)
            .join(format!("{year}.db"))
    }

    /// Content-addressed immutable raw objects.
    pub fn objects(&self) -> PathBuf {
        match &self.scope {
            Some(scope) => self.root.join("objects").join(scope).join("sha256"),
            None => self.root.join("objects/sha256"),
        }
    }

    /// Deterministic sharded path for one immutable object.
    pub fn object_path(&self, digest: Sha256Digest) -> PathBuf {
        let hex = digest.to_hex();
        self.objects().join(&hex[..2]).join(hex)
    }

    pub(crate) fn temporary_download(&self, id: &str) -> PathBuf {
        self.temporary_root().join(format!("download-{id}.part"))
    }

    pub(crate) fn temporary_root(&self) -> PathBuf {
        match &self.scope {
            Some(scope) => self.root.join("tmp").join(scope),
            None => self.root.join("tmp"),
        }
    }

    pub(crate) fn scoped_name(&self) -> Option<&str> {
        self.scope.as_deref()
    }

    pub(crate) fn initialize(&self) -> Result<(), crate::StoreError> {
        let directories = [
            self.objects(),
            self.root.join("indices"),
            self.temporary_root(),
        ];
        directories.into_iter().try_for_each(|directory| {
            fs::create_dir_all(&directory).map_err(|error| crate::StoreError::io(directory, error))
        })?;
        self.cleanup_stale_temporary()
    }

    fn cleanup_stale_temporary(&self) -> Result<(), crate::StoreError> {
        let temporary = self.temporary_root();
        let now = std::time::SystemTime::now();
        for entry in
            fs::read_dir(&temporary).map_err(|error| crate::StoreError::io(&temporary, error))?
        {
            let entry = entry.map_err(|error| crate::StoreError::io(&temporary, error))?;
            let path = entry.path();
            let owned_temporary =
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("download-")
                            && Path::new(name)
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("part"))
                    });
            if !owned_temporary {
                continue;
            }
            let metadata = entry
                .metadata()
                .map_err(|error| crate::StoreError::io(&path, error))?;
            let stale = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= std::time::Duration::from_secs(24 * 60 * 60));
            if stale && metadata.is_file() {
                fs::remove_file(&path).map_err(|error| crate::StoreError::io(path, error))?;
            }
        }
        Ok(())
    }
}
