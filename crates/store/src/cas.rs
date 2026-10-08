use std::path::{Path, PathBuf};

use ionoray_core::Sha256Digest;
use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, File, OpenOptions},
    io::{AsyncReadExt, AsyncWriteExt},
};
use uuid::Uuid;

use crate::{
    StoreLayout,
    download::DownloadError,
    time::{system_time_nanos, unix_time_millis},
};

pub(crate) struct Cas<'a> {
    layout: &'a StoreLayout,
}

impl<'a> Cas<'a> {
    pub(crate) const fn new(layout: &'a StoreLayout) -> Self {
        Self { layout }
    }

    pub(crate) async fn ingest_response(
        &self,
        mut response: reqwest::Response,
        expected_length: Option<u64>,
    ) -> Result<StoredObject, DownloadError> {
        let temporary_id = Uuid::now_v7().as_hyphenated().to_string();
        let temporary = self.layout.temporary_download(&temporary_id);
        let mut file = create_temporary(&temporary).await?;
        let streamed = stream_response(&mut file, &mut response, &temporary).await;
        drop(file);
        let (digest, byte_size) = match streamed {
            Ok(result) => result,
            Err(error) => {
                best_effort_remove(&temporary).await;
                return Err(error);
            }
        };

        if let Some(expected) = expected_length
            && expected != byte_size
        {
            best_effort_remove(&temporary).await;
            return Err(DownloadError::ContentLengthMismatch {
                expected,
                actual: byte_size,
            });
        }

        self.commit(temporary, digest, byte_size).await
    }

    pub(crate) async fn ingest_bytes(&self, bytes: &[u8]) -> Result<StoredObject, DownloadError> {
        let temporary_id = Uuid::now_v7().as_hyphenated().to_string();
        let temporary = self.layout.temporary_download(&temporary_id);
        let mut file = create_temporary(&temporary).await?;
        if let Err(error) = file.write_all(bytes).await {
            drop(file);
            best_effort_remove(&temporary).await;
            return Err(DownloadError::io(&temporary, error));
        }
        file.flush()
            .await
            .map_err(|error| DownloadError::io(&temporary, error))?;
        file.sync_all()
            .await
            .map_err(|error| DownloadError::io(&temporary, error))?;
        drop(file);
        let byte_size = u64::try_from(bytes.len())
            .map_err(|_| DownloadError::NumericOverflow("bundled byte count"))?;
        let digest = Sha256Digest::from_bytes(Sha256::digest(bytes).into());
        self.commit(temporary, digest, byte_size).await
    }

    pub(crate) async fn verified_object(
        &self,
        digest: Sha256Digest,
        expected_size: u64,
        expected_mtime_ns: i64,
        downloaded_at_utc_ms: i64,
        force_hash: bool,
    ) -> Result<Option<StoredObject>, DownloadError> {
        let path = self.layout.object_path(digest);
        let metadata = match fs::symlink_metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(DownloadError::io(&path, error)),
        };
        if !metadata.file_type().is_file() {
            return Err(DownloadError::InvalidObjectType(path));
        }
        let actual_mtime_ns = metadata
            .modified()
            .map(system_time_nanos)
            .map_err(|error| DownloadError::io(&path, error))?;
        if metadata.len() != expected_size
            || ((force_hash || actual_mtime_ns != expected_mtime_ns)
                && hash_file(&path).await? != digest)
        {
            return Err(DownloadError::CorruptObject {
                path,
                expected: digest,
            });
        }
        Ok(Some(StoredObject::from_metadata(
            self.layout,
            path,
            digest,
            &metadata,
            false,
            downloaded_at_utc_ms,
        )?))
    }

    pub(crate) async fn discard_corrupt(&self, path: &Path) -> Result<(), DownloadError> {
        let mut permissions = fs::metadata(path)
            .await
            .map_err(|error| DownloadError::io(path, error))?
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)
            .await
            .map_err(|error| DownloadError::io(path, error))?;
        fs::remove_file(path)
            .await
            .map_err(|error| DownloadError::io(path, error))
    }

    async fn commit(
        &self,
        temporary: PathBuf,
        digest: Sha256Digest,
        byte_size: u64,
    ) -> Result<StoredObject, DownloadError> {
        let target = self.layout.object_path(digest);
        let parent = target
            .parent()
            .expect("CAS object paths always have a shard directory");
        fs::create_dir_all(parent)
            .await
            .map_err(|error| DownloadError::io(parent, error))?;

        let created = match fs::hard_link(&temporary, &target).await {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = object_metadata(&target).await?;
                if metadata.len() != byte_size || hash_file(&target).await? != digest {
                    best_effort_remove(&temporary).await;
                    return Err(DownloadError::CorruptObject {
                        path: target,
                        expected: digest,
                    });
                }
                false
            }
            Err(error) => {
                best_effort_remove(&temporary).await;
                return Err(DownloadError::io(&target, error));
            }
        };
        best_effort_remove(&temporary).await;

        if created {
            let mut permissions = fs::metadata(&target)
                .await
                .map_err(|error| DownloadError::io(&target, error))?
                .permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&target, permissions)
                .await
                .map_err(|error| DownloadError::io(&target, error))?;
        }

        let metadata = object_metadata(&target).await?;
        StoredObject::from_metadata(
            self.layout,
            target,
            digest,
            &metadata,
            created,
            unix_time_millis(),
        )
    }
}

pub(crate) struct StoredObject {
    pub(crate) digest: Sha256Digest,
    pub(crate) byte_size: u64,
    pub(crate) path: PathBuf,
    pub(crate) relative_path: String,
    pub(crate) local_mtime_ns: i64,
    pub(crate) downloaded_at_utc_ms: i64,
    pub(crate) created: bool,
}

impl StoredObject {
    fn from_metadata(
        layout: &StoreLayout,
        path: PathBuf,
        digest: Sha256Digest,
        metadata: &std::fs::Metadata,
        created: bool,
        downloaded_at_utc_ms: i64,
    ) -> Result<Self, DownloadError> {
        let relative = path
            .strip_prefix(layout.root())
            .expect("CAS paths are rooted under the store");
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let local_mtime_ns = metadata
            .modified()
            .map(system_time_nanos)
            .map_err(|error| DownloadError::io(&path, error))?;
        Ok(Self {
            digest,
            byte_size: metadata.len(),
            path,
            relative_path,
            local_mtime_ns,
            downloaded_at_utc_ms,
            created,
        })
    }
}

async fn create_temporary(path: &Path) -> Result<File, DownloadError> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .await
        .map_err(|error| DownloadError::io(path, error))
}

async fn hash_file(path: &Path) -> Result<Sha256Digest, DownloadError> {
    let mut file = File::open(path)
        .await
        .map_err(|error| DownloadError::io(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| DownloadError::io(path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_bytes(hasher.finalize().into()))
}

async fn stream_response(
    file: &mut File,
    response: &mut reqwest::Response,
    path: &Path,
) -> Result<(Sha256Digest, u64), DownloadError> {
    let mut hasher = Sha256::new();
    let mut byte_size = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)
            .await
            .map_err(|error| DownloadError::io(path, error))?;
        hasher.update(&chunk);
        let chunk_size = u64::try_from(chunk.len())
            .map_err(|_| DownloadError::NumericOverflow("download chunk size"))?;
        byte_size = byte_size
            .checked_add(chunk_size)
            .ok_or(DownloadError::NumericOverflow("download byte count"))?;
    }
    file.flush()
        .await
        .map_err(|error| DownloadError::io(path, error))?;
    file.sync_all()
        .await
        .map_err(|error| DownloadError::io(path, error))?;
    Ok((
        Sha256Digest::from_bytes(hasher.finalize().into()),
        byte_size,
    ))
}

async fn object_metadata(path: &Path) -> Result<std::fs::Metadata, DownloadError> {
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(|error| DownloadError::io(path, error))?;
    if !metadata.file_type().is_file() {
        return Err(DownloadError::InvalidObjectType(path.to_path_buf()));
    }
    Ok(metadata)
}

async fn best_effort_remove(path: &Path) {
    let _ = fs::remove_file(path).await;
}
