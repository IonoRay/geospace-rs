use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ionoray_core::Sha256Digest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use turso::{Database, params};
use url::Url;

use crate::{ArtifactRef, DownloadRequest, StoreError, StoreLayout, view::browse_collision};

/// A validated, domain-neutral name for an isolated dataset object store.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct StoreScope(String);

impl StoreScope {
    /// Validates a portable dataset scope name.
    ///
    /// # Errors
    /// Returns an error for an unsafe scope name.
    pub fn new(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 80
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(StoreError::InvalidScope(value));
        }
        Ok(Self(value))
    }

    /// Returns the validated scope name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An artifact whose bytes passed dataset-owned domain validation and became a baseline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AcceptedArtifact {
    /// The immutable local body accepted by the dataset layer.
    pub artifact: ArtifactRef,
    /// Local UTC time when the dataset accepted the body.
    pub accepted_at_utc_ms: i64,
    /// Managed relative symlink used for human browsing.
    pub browse_path: PathBuf,
}

/// A per-dataset catalog, CAS, temporary area, and cross-process guards.
pub struct ScopedStore {
    pub(crate) scope: StoreScope,
    pub(crate) layout: StoreLayout,
    pub(crate) catalog: Database,
}

impl ScopedStore {
    pub(crate) fn new(scope: StoreScope, layout: StoreLayout, catalog: Database) -> Self {
        Self {
            scope,
            layout,
            catalog,
        }
    }

    /// The isolated dataset scope.
    pub fn scope(&self) -> &StoreScope {
        &self.scope
    }

    /// Paths rooted at the parent store home.
    pub fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    /// Records a dataset-validated candidate as the current accepted baseline.
    ///
    /// This function deliberately performs no scientific validation. The caller
    /// must validate `artifact` before calling it.
    ///
    /// # Errors
    /// Returns an error for an invalid candidate, catalog, or view operation.
    ///
    /// # Panics
    /// Panics only if the internal scoped object layout has no parent directory.
    pub async fn accept(
        &self,
        request: &DownloadRequest,
        artifact: &ArtifactRef,
    ) -> Result<AcceptedArtifact, StoreError> {
        let expected = self.layout.object_path(artifact.digest);
        if artifact.path != expected
            || fs::metadata(&expected)
                .map_err(|error| StoreError::io(&expected, error))?
                .len()
                != artifact.byte_size
        {
            return Err(StoreError::InvalidAcceptedArtifact(expected));
        }
        let mut accepted_at_utc_ms = unix_time_millis()?;
        let mut browse_relative = browse_relative(
            request.url(),
            request.logical_name(),
            artifact.digest,
            accepted_at_utc_ms,
        )?;
        let mut connection = self.catalog.connect()?;
        let transaction = connection.transaction().await?;
        let mut accepted_rows = transaction
            .query(
                "SELECT artifact_sha256, browse_relative, accepted_at_utc_ms FROM accepted_source WHERE source_id = ?",
                [request.source_id()],
            )
            .await?;
        if let Some(row) = accepted_rows.next().await? {
            let existing_digest: String = row.get(0)?;
            if existing_digest == artifact.digest.to_string() {
                accepted_at_utc_ms = row.get(2)?;
                browse_relative = safe_relative(PathBuf::from(row.get::<String>(1)?))?;
            }
        }
        drop(accepted_rows);
        let mut sequence = 2_u32;
        loop {
            let mut view_rows = transaction
                .query(
                    "SELECT artifact_sha256 FROM accepted_view WHERE browse_relative = ?",
                    [browse_relative.to_string_lossy().as_ref()],
                )
                .await?;
            let existing = view_rows
                .next()
                .await?
                .map(|row| row.get::<String>(0))
                .transpose()?;
            drop(view_rows);
            match existing {
                None => break,
                Some(digest) if digest == artifact.digest.to_string() => break,
                Some(_) => {
                    browse_relative = browse_collision(&browse_relative, sequence)?;
                    sequence = sequence.saturating_add(1);
                }
            }
        }
        let browse_path = self
            .layout
            .objects()
            .parent()
            .expect("scoped objects have a parent")
            .join(&browse_relative);
        let mut rows = transaction
            .query(
                "SELECT observation_id FROM fetch_attempt WHERE source_id = ? AND artifact_sha256 = ? ORDER BY queried_at_utc_ms DESC, observation_id DESC LIMIT 1",
                params![request.source_id(), artifact.digest.to_string()],
            )
            .await?;
        let observation_id: String = rows
            .next()
            .await?
            .ok_or_else(|| StoreError::InvalidAcceptedArtifact(expected.clone()))?
            .get(0)?;
        transaction.execute(
            "INSERT INTO accepted_source (source_id, artifact_sha256, observation_id, accepted_at_utc_ms, canonical_url, browse_relative) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(source_id) DO UPDATE SET pending_previous_sha256 = CASE WHEN accepted_source.artifact_sha256 <> excluded.artifact_sha256 THEN accepted_source.artifact_sha256 ELSE accepted_source.pending_previous_sha256 END, artifact_sha256 = excluded.artifact_sha256, observation_id = excluded.observation_id, accepted_at_utc_ms = excluded.accepted_at_utc_ms, canonical_url = excluded.canonical_url, browse_relative = excluded.browse_relative",
            params![request.source_id(), artifact.digest.to_string(), observation_id, accepted_at_utc_ms, request.url().as_str(), browse_relative.to_string_lossy().as_ref()],
        ).await?;
        transaction.execute("INSERT INTO accepted_view (browse_relative, source_id, artifact_sha256, accepted_at_utc_ms) VALUES (?, ?, ?, ?) ON CONFLICT(browse_relative) DO NOTHING", params![browse_relative.to_string_lossy().as_ref(), request.source_id(), artifact.digest.to_string(), accepted_at_utc_ms]).await?;
        transaction.commit().await?;
        create_view(&self.layout, &browse_relative, artifact.digest)?;
        Ok(AcceptedArtifact {
            artifact: artifact.clone(),
            accepted_at_utc_ms,
            browse_path,
        })
    }

    /// Recreates every accepted browsing symlink without using the network.
    ///
    /// # Errors
    /// Returns an error for a missing, corrupt, or unsafe managed object/view.
    pub async fn repair_views(&self) -> Result<usize, StoreError> {
        let connection = self.catalog.connect()?;
        let mut rows = connection
            .query(
                "SELECT browse_relative, artifact_sha256 FROM accepted_view ORDER BY browse_relative",
                (),
            )
            .await?;
        let mut repaired = 0;
        while let Some(row) = rows.next().await? {
            let relative = safe_relative(PathBuf::from(row.get::<String>(0)?))?;
            let digest: Sha256Digest = row
                .get::<String>(1)?
                .parse()
                .map_err(|_| StoreError::InvalidBrowsePath(relative.clone()))?;
            let object = self.layout.object_path(digest);
            verify_object(&object, digest)?;
            create_view(&self.layout, &relative, digest)?;
            repaired += 1;
        }
        Ok(repaired)
    }

    /// Attempts to exclude other synchronizers for this dataset scope.
    ///
    /// # Errors
    /// Returns `ScopeBusy` when another process holds a read or write lock.
    pub fn try_acquire_sync_lock(&self) -> Result<SyncGuard, StoreError> {
        let path = lock_path(&self.layout)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| StoreError::io(&path, error))?;
        file.try_lock()
            .map_err(|_| StoreError::ScopeBusy(self.scope.0.clone()))?;
        Ok(SyncGuard { path, file })
    }

    /// Prevents a concurrently acquired synchronizer from cleaning accepted objects.
    ///
    /// # Errors
    /// Returns `ScopeBusy` when another process holds the writer lock.
    pub fn try_acquire_read_guard(&self) -> Result<ReadGuard, StoreError> {
        let path = lock_path(&self.layout)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| StoreError::io(&path, error))?;
        file.try_lock_shared()
            .map_err(|_| StoreError::ScopeBusy(self.scope.0.clone()))?;
        Ok(ReadGuard { file })
    }
}

/// Cross-process synchronization guard. Removing it releases the scope.
pub struct SyncGuard {
    pub(crate) path: PathBuf,
    file: File,
}
impl Drop for SyncGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Cross-process read guard. Removing it permits a synchronizer to proceed.
pub struct ReadGuard {
    file: File,
}
impl Drop for ReadGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn lock_path(layout: &StoreLayout) -> Result<PathBuf, StoreError> {
    let locks = layout.temporary_root().join("locks");
    fs::create_dir_all(&locks).map_err(|error| StoreError::io(&locks, error))?;
    Ok(locks.join("scope.lock"))
}

fn unix_time_millis() -> Result<i64, StoreError> {
    Ok(i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| StoreError::Clock(error.duration()))?
            .as_millis(),
    )
    .unwrap_or(i64::MAX))
}

fn browse_relative(
    url: &Url,
    filename: &str,
    _digest: Sha256Digest,
    millis: i64,
) -> Result<PathBuf, StoreError> {
    let host = url
        .host_str()
        .ok_or_else(|| StoreError::InvalidCanonicalUrl(url.to_string()))?;
    let mut relative = PathBuf::new();
    relative.push(encode(host));
    let mut segments: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect();
    // The last URL component is normally the remote filename. `logical_name`
    // owns the portable filename shown in the browse view.
    segments.pop();
    for segment in segments {
        relative.push(encode(segment));
    }
    if let Some(query) = url.query() {
        relative.push(format!("q-{}", &query_hash_prefix(query)[..12]));
    }
    relative.push(format!("{}__{}", timestamp(millis), encode(filename)));
    if relative.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StoreError::InvalidBrowsePath(relative));
    }
    Ok(relative)
}

fn create_view(
    layout: &StoreLayout,
    relative: &Path,
    digest: Sha256Digest,
) -> Result<(), StoreError> {
    let _scope = layout
        .scoped_name()
        .ok_or_else(|| StoreError::InvalidBrowsePath(relative.to_path_buf()))?;
    let link = layout
        .objects()
        .parent()
        .expect("objects have parent")
        .join(relative);
    let target = layout.object_path(digest);
    let parent = link
        .parent()
        .ok_or_else(|| StoreError::InvalidBrowsePath(link.clone()))?;
    ensure_managed_parent(
        layout.objects().parent().expect("objects have parent"),
        parent,
    )?;
    let target_relative = relative_target(parent, &target)?;
    match fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(StoreError::RefuseOverwriteView(link)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StoreError::io(&link, error)),
    }
    let temporary = parent.join(format!(".view-{}.tmp", uuid::Uuid::now_v7()));
    #[cfg(unix)]
    std::os::unix::fs::symlink(target_relative, &temporary)
        .map_err(|error| StoreError::io(&temporary, error))?;
    #[cfg(not(unix))]
    std::os::windows::fs::symlink_file(target_relative, &temporary)
        .map_err(|error| StoreError::io(&temporary, error))?;
    fs::rename(&temporary, &link).map_err(|error| StoreError::io(&link, error))?;
    Ok(())
}

fn ensure_managed_parent(root: &Path, target: &Path) -> Result<(), StoreError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| StoreError::InvalidBrowsePath(target.to_path_buf()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err(StoreError::InvalidBrowsePath(current)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| StoreError::io(&current, error))?;
            }
            Err(error) => return Err(StoreError::io(&current, error)),
        }
    }
    Ok(())
}

fn query_hash_prefix(query: &str) -> String {
    use std::fmt::Write;
    Sha256::digest(query.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            write!(&mut text, "{byte:02x}").expect("writing to String");
            text
        })
}

fn relative_target(parent: &Path, target: &Path) -> Result<PathBuf, StoreError> {
    let parent = parent
        .canonicalize()
        .map_err(|error| StoreError::io(parent, error))?;
    let target_parent = target
        .parent()
        .ok_or_else(|| StoreError::InvalidBrowsePath(target.to_path_buf()))?;
    let mut result = PathBuf::new();
    let parent_parts: Vec<_> = parent.components().collect();
    let target_parts: Vec<_> = target_parent.components().collect();
    let common = parent_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(a, b)| a == b)
        .count();
    for _ in common..parent_parts.len() {
        result.push("..");
    }
    for part in &target_parts[common..] {
        result.push(part.as_os_str());
    }
    result.push(
        target
            .file_name()
            .ok_or_else(|| StoreError::InvalidBrowsePath(target.to_path_buf()))?,
    );
    Ok(result)
}

fn verify_object(path: &Path, expected: Sha256Digest) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| StoreError::io(path, error))?;
    if !metadata.file_type().is_file() {
        return Err(StoreError::InvalidAcceptedArtifact(path.to_path_buf()));
    }
    let mut file = File::open(path).map_err(|error| StoreError::io(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| StoreError::io(path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if Sha256Digest::from_bytes(hasher.finalize().into()) == expected {
        Ok(())
    } else {
        Err(StoreError::InvalidAcceptedArtifact(path.to_path_buf()))
    }
}

fn safe_relative(path: PathBuf) -> Result<PathBuf, StoreError> {
    if path.is_relative()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        Ok(path)
    } else {
        Err(StoreError::InvalidBrowsePath(path))
    }
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || b"._-".contains(&byte) {
                vec![char::from(byte)]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}
fn timestamp(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let millis = millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3600;
    let minute = (second_of_day % 3600) / 60;
    let second = second_of_day % 60;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}.{millis:03}Z")
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
#[path = "scope/tests.rs"]
mod tests;
