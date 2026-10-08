use ionoray_core::Sha256Digest;
use turso::{Database, params};

use crate::{
    cas::StoredObject,
    database::checkpoint,
    download::{
        ArtifactRef, DownloadError, DownloadObservation, DownloadRequest, ObservationStatus,
    },
};

pub(crate) struct DownloadRepository<'a> {
    pub(super) catalog: &'a Database,
    pub(super) accepted_baseline: bool,
}

impl<'a> DownloadRepository<'a> {
    pub(crate) const fn new(catalog: &'a Database) -> Self {
        Self {
            catalog,
            accepted_baseline: false,
        }
    }

    pub(crate) fn use_accepted_baseline(&mut self) {
        self.accepted_baseline = true;
    }

    pub(crate) async fn latest_baseline(
        &self,
        source_id: &str,
    ) -> Result<Option<PreviousArtifact>, DownloadError> {
        if self.accepted_baseline {
            self.latest_accepted(source_id).await
        } else {
            self.latest_success(source_id).await
        }
    }

    pub(crate) async fn begin(
        &self,
        observation_id: &str,
        request: &DownloadRequest,
        check_mode: &str,
        queried_at_utc_ms: i64,
        previous: Option<&PreviousArtifact>,
    ) -> Result<(), DownloadError> {
        let context = request.context();
        let mut connection = self.catalog.connect()?;
        let transaction = connection.transaction().await?;
        transaction
            .execute(
                "INSERT INTO source_object (source_id, provider, dataset, year, month, edition, priority, canonical_url, logical_name, created_at_utc_ms, updated_at_utc_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(source_id) DO UPDATE SET provider = excluded.provider, dataset = excluded.dataset, year = excluded.year, month = excluded.month, edition = excluded.edition, priority = excluded.priority, canonical_url = excluded.canonical_url, logical_name = excluded.logical_name, updated_at_utc_ms = excluded.updated_at_utc_ms",
                params![
                    request.source_id(),
                    context.map(|item| item.provider.as_str()),
                    context.map(|item| item.dataset.as_str()),
                    context.map(|item| i64::from(item.year)),
                    context.and_then(|item| item.month.map(i64::from)),
                    context.map(|item| item.edition.as_str()),
                    context.map(|item| i64::from(item.priority)),
                    request.url().as_str(),
                    request.logical_name(),
                    queried_at_utc_ms,
                    queried_at_utc_ms,
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO fetch_attempt (observation_id, source_id, request_url, check_mode, queried_at_utc_ms, request_etag, request_last_modified, status) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    observation_id,
                    request.source_id(),
                    request.url().as_str(),
                    check_mode,
                    queried_at_utc_ms,
                    previous.and_then(|item| item.etag.as_deref()),
                    previous.and_then(|item| item.last_modified_raw.as_deref()),
                    ObservationStatus::Querying.as_str(),
                ],
            )
            .await?;
        transaction.commit().await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        Ok(())
    }

    pub(crate) async fn start_download(
        &self,
        observation_id: &str,
        started_at_utc_ms: i64,
    ) -> Result<(), DownloadError> {
        self.catalog
            .connect()?
            .execute(
                "UPDATE fetch_attempt SET download_started_at_utc_ms = ? WHERE observation_id = ? AND status = 'querying'",
                params![started_at_utc_ms, observation_id],
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn complete_without_body(
        &self,
        completed: &WithoutBodyCompletion<'_>,
    ) -> Result<(), DownloadError> {
        let content_length = optional_i64(completed.response.content_length, "content length")?;
        let mut connection = self.catalog.connect()?;
        let transaction = connection.transaction().await?;
        transaction
            .execute(
                "UPDATE artifact SET last_verified_at_utc_ms = ?, local_mtime_ns = ? WHERE sha256 = ?",
                params![completed.finished_at_utc_ms, completed.object.local_mtime_ns, completed.previous.digest.to_string()],
            )
            .await?;
        transaction
            .execute(
                "UPDATE fetch_attempt SET download_finished_at_utc_ms = ?, final_url = ?, http_status = ?, response_etag = ?, last_modified_raw = ?, last_modified_utc_ms = ?, content_length_header = ?, bytes_received = 0, artifact_sha256 = ?, status = ? WHERE observation_id = ?",
                params![
                    completed.finished_at_utc_ms,
                    completed.response.final_url.as_str(),
                    i64::from(completed.response.http_status),
                    completed.response.etag.as_deref().or(completed.previous.etag.as_deref()),
                    completed.response.last_modified_raw.as_deref().or(completed.previous.last_modified_raw.as_deref()),
                    completed.response.last_modified_utc_ms.or(completed.previous.source_modified_at_utc_ms),
                    content_length,
                    completed.previous.digest.to_string(),
                    completed.status.as_str(),
                    completed.observation_id,
                ],
            )
            .await?;
        upsert_origin(
            &transaction,
            completed.observation_id,
            completed.request,
            completed.previous.digest,
            completed.response.final_url.as_str(),
            completed.response.content_disposition.as_deref(),
            completed.finished_at_utc_ms,
        )
        .await?;
        transaction.commit().await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        Ok(())
    }

    pub(crate) async fn complete_download(
        &self,
        observation_id: &str,
        request: &DownloadRequest,
        object: &StoredObject,
        response: &ResponseMetadata,
        finished_at_utc_ms: i64,
    ) -> Result<i64, DownloadError> {
        let byte_size = to_i64(object.byte_size, "artifact byte size")?;
        let content_length = optional_i64(response.content_length, "content length")?;
        let status = if object.created {
            ObservationStatus::Downloaded
        } else {
            ObservationStatus::Unchanged
        };
        let mut connection = self.catalog.connect()?;
        let transaction = connection.transaction().await?;
        transaction
            .execute(
                "INSERT INTO artifact (sha256, byte_size, relative_path, local_mtime_ns, downloaded_at_utc_ms, first_seen_at_utc_ms, last_verified_at_utc_ms, original_filename, media_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(sha256) DO UPDATE SET local_mtime_ns = excluded.local_mtime_ns, last_verified_at_utc_ms = excluded.last_verified_at_utc_ms, media_type = COALESCE(artifact.media_type, excluded.media_type)",
                params![
                    object.digest.to_string(),
                    byte_size,
                    object.relative_path.as_str(),
                    object.local_mtime_ns,
                    object.downloaded_at_utc_ms,
                    object.downloaded_at_utc_ms,
                    finished_at_utc_ms,
                    request.logical_name(),
                    response.media_type.as_deref(),
                ],
            )
            .await?;
        transaction
            .execute(
                "UPDATE fetch_attempt SET download_finished_at_utc_ms = ?, final_url = ?, http_status = ?, response_etag = ?, last_modified_raw = ?, last_modified_utc_ms = ?, content_length_header = ?, bytes_received = ?, artifact_sha256 = ?, status = ? WHERE observation_id = ?",
                params![
                    finished_at_utc_ms,
                    response.final_url.as_str(),
                    i64::from(response.http_status),
                    response.etag.as_deref(),
                    response.last_modified_raw.as_deref(),
                    response.last_modified_utc_ms,
                    content_length,
                    byte_size,
                    object.digest.to_string(),
                    status.as_str(),
                    observation_id,
                ],
            )
            .await?;
        upsert_origin(
            &transaction,
            observation_id,
            request,
            object.digest,
            response.final_url.as_str(),
            response.content_disposition.as_deref(),
            finished_at_utc_ms,
        )
        .await?;
        transaction.commit().await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        self.persisted_downloaded_at(object.digest).await
    }

    pub(crate) async fn complete_bundled(
        &self,
        observation_id: &str,
        request: &DownloadRequest,
        object: &StoredObject,
        source_modified_at_utc_ms: i64,
        finished_at_utc_ms: i64,
    ) -> Result<i64, DownloadError> {
        let byte_size = to_i64(object.byte_size, "artifact byte size")?;
        let status = if object.created {
            ObservationStatus::Downloaded
        } else {
            ObservationStatus::Unchanged
        };
        let mut connection = self.catalog.connect()?;
        let transaction = connection.transaction().await?;
        transaction
            .execute(
                "INSERT INTO artifact (sha256, byte_size, relative_path, local_mtime_ns, downloaded_at_utc_ms, first_seen_at_utc_ms, last_verified_at_utc_ms, original_filename, media_type) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'text/plain') ON CONFLICT(sha256) DO UPDATE SET local_mtime_ns = excluded.local_mtime_ns, last_verified_at_utc_ms = excluded.last_verified_at_utc_ms, media_type = COALESCE(artifact.media_type, excluded.media_type)",
                params![
                    object.digest.to_string(),
                    byte_size,
                    object.relative_path.as_str(),
                    object.local_mtime_ns,
                    object.downloaded_at_utc_ms,
                    object.downloaded_at_utc_ms,
                    finished_at_utc_ms,
                    request.logical_name(),
                ],
            )
            .await?;
        transaction
            .execute(
                "UPDATE fetch_attempt SET download_finished_at_utc_ms = ?, final_url = ?, http_status = NULL, last_modified_utc_ms = ?, content_length_header = ?, bytes_received = ?, artifact_sha256 = ?, status = ? WHERE observation_id = ?",
                params![
                    finished_at_utc_ms,
                    request.url().as_str(),
                    source_modified_at_utc_ms,
                    byte_size,
                    byte_size,
                    object.digest.to_string(),
                    status.as_str(),
                    observation_id,
                ],
            )
            .await?;
        upsert_origin(
            &transaction,
            observation_id,
            request,
            object.digest,
            request.url().as_str(),
            None,
            finished_at_utc_ms,
        )
        .await?;
        transaction.commit().await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        self.persisted_downloaded_at(object.digest).await
    }

    async fn persisted_downloaded_at(&self, digest: Sha256Digest) -> Result<i64, DownloadError> {
        let connection = self.catalog.connect()?;
        let digest = digest.to_string();
        let mut rows = connection
            .query(
                "SELECT downloaded_at_utc_ms FROM artifact WHERE sha256 = ?",
                [digest.as_str()],
            )
            .await?;
        rows.next()
            .await?
            .ok_or(DownloadError::MissingObservationField(
                "artifact downloaded_at",
            ))?
            .get(0)
            .map_err(Into::into)
    }

    pub(crate) async fn complete_query(
        &self,
        observation_id: &str,
        response: &ResponseMetadata,
        finished_at_utc_ms: i64,
        status: ObservationStatus,
    ) -> Result<(), DownloadError> {
        let content_length = optional_i64(response.content_length, "content length")?;
        self.catalog
            .connect()?
            .execute(
                "UPDATE fetch_attempt SET download_finished_at_utc_ms = ?, final_url = ?, http_status = ?, response_etag = ?, last_modified_raw = ?, last_modified_utc_ms = ?, content_length_header = ?, bytes_received = 0, status = ? WHERE observation_id = ?",
                params![finished_at_utc_ms, response.final_url.as_str(), i64::from(response.http_status), response.etag.as_deref(), response.last_modified_raw.as_deref(), response.last_modified_utc_ms, content_length, status.as_str(), observation_id],
            )
            .await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        Ok(())
    }

    pub(crate) async fn fail(
        &self,
        observation_id: &str,
        error_kind: &str,
        message: &str,
        finished_at_utc_ms: i64,
        http_status: Option<u16>,
    ) -> Result<(), DownloadError> {
        self.catalog
            .connect()?
            .execute(
                "UPDATE fetch_attempt SET download_finished_at_utc_ms = ?, http_status = ?, status = ?, error_kind = ?, error_message = ? WHERE observation_id = ?",
                params![finished_at_utc_ms, http_status.map(i64::from), ObservationStatus::Failed.as_str(), error_kind, message, observation_id],
            )
            .await?;
        checkpoint(self.catalog, "objects/catalog.db").await?;
        Ok(())
    }

    pub(crate) async fn history(
        &self,
        source_id: &str,
    ) -> Result<Vec<DownloadObservation>, DownloadError> {
        let connection = self.catalog.connect()?;
        let mut rows = connection
            .query(
                "SELECT observation_id, source_id, request_url, queried_at_utc_ms, download_started_at_utc_ms, download_finished_at_utc_ms, request_etag, request_last_modified, final_url, http_status, response_etag, last_modified_raw, last_modified_utc_ms, content_length_header, bytes_received, artifact_sha256, status, error_kind, error_message FROM fetch_attempt WHERE source_id = ? ORDER BY queried_at_utc_ms, observation_id",
                [source_id],
            )
            .await?;
        let mut observations = Vec::new();
        while let Some(row) = rows.next().await? {
            let digest = row
                .get::<Option<String>>(15)?
                .map(|value| value.parse())
                .transpose()?;
            let status_text: String = row.get(16)?;
            observations.push(DownloadObservation {
                observation_id: row.get(0)?,
                source_id: row.get(1)?,
                request_url: row.get(2)?,
                queried_at_utc_ms: row.get(3)?,
                download_started_at_utc_ms: row.get(4)?,
                download_finished_at_utc_ms: row.get(5)?,
                request_etag: row.get(6)?,
                request_last_modified: row.get(7)?,
                final_url: row.get(8)?,
                http_status: row.get::<Option<i64>>(9)?.map(to_u16).transpose()?,
                response_etag: row.get(10)?,
                last_modified_raw: row.get(11)?,
                last_modified_utc_ms: row.get(12)?,
                content_length_header: optional_u64(row.get(13)?, "content length")?,
                bytes_received: optional_u64(row.get(14)?, "bytes received")?,
                artifact: digest,
                status: ObservationStatus::from_str(&status_text)?,
                error_kind: row.get(17)?,
                error_message: row.get(18)?,
            });
        }
        Ok(observations)
    }
}

pub(crate) struct WithoutBodyCompletion<'a> {
    pub(crate) observation_id: &'a str,
    pub(crate) request: &'a DownloadRequest,
    pub(crate) previous: &'a PreviousArtifact,
    pub(crate) response: &'a ResponseMetadata,
    pub(crate) object: &'a StoredObject,
    pub(crate) finished_at_utc_ms: i64,
    pub(crate) status: ObservationStatus,
}

async fn upsert_origin(
    transaction: &turso::transaction::Transaction<'_>,
    observation_id: &str,
    request: &DownloadRequest,
    digest: Sha256Digest,
    final_url: &str,
    content_disposition: Option<&str>,
    observed_at: i64,
) -> Result<(), DownloadError> {
    transaction
        .execute(
            "INSERT INTO artifact_origin (artifact_sha256, source_id, original_filename, content_disposition, final_url, first_observation_id, last_observation_id, first_observed_at_utc_ms, last_observed_at_utc_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(artifact_sha256, source_id) DO UPDATE SET original_filename = excluded.original_filename, content_disposition = COALESCE(excluded.content_disposition, artifact_origin.content_disposition), final_url = excluded.final_url, last_observation_id = excluded.last_observation_id, last_observed_at_utc_ms = excluded.last_observed_at_utc_ms",
            params![digest.to_string(), request.source_id(), request.logical_name(), content_disposition, final_url, observation_id, observation_id, observed_at, observed_at],
        )
        .await?;
    Ok(())
}

pub(crate) struct PreviousArtifact {
    pub(crate) digest: Sha256Digest,
    pub(crate) byte_size: u64,
    pub(crate) original_filename: String,
    pub(crate) media_type: Option<String>,
    pub(crate) source_modified_at_utc_ms: Option<i64>,
    pub(crate) etag: Option<String>,
    pub(crate) last_modified_raw: Option<String>,
    pub(crate) final_url: Option<String>,
    pub(crate) downloaded_at_utc_ms: i64,
    pub(crate) local_mtime_ns: i64,
}

impl PreviousArtifact {
    pub(crate) fn to_artifact_ref(&self, object: &StoredObject) -> ArtifactRef {
        ArtifactRef {
            digest: self.digest,
            byte_size: self.byte_size,
            path: object.path.clone(),
            original_filename: self.original_filename.clone(),
            media_type: self.media_type.clone(),
            source_modified_at_utc_ms: self.source_modified_at_utc_ms,
            downloaded_at_utc_ms: self.downloaded_at_utc_ms,
            local_mtime_ns: object.local_mtime_ns,
        }
    }
}

pub(crate) struct ResponseMetadata {
    pub(crate) final_url: String,
    pub(crate) http_status: u16,
    pub(crate) etag: Option<String>,
    pub(crate) last_modified_raw: Option<String>,
    pub(crate) last_modified_utc_ms: Option<i64>,
    pub(crate) content_length: Option<u64>,
    pub(crate) media_type: Option<String>,
    pub(crate) content_disposition: Option<String>,
}

fn optional_i64(value: Option<u64>, field: &'static str) -> Result<Option<i64>, DownloadError> {
    value.map(|item| to_i64(item, field)).transpose()
}

fn optional_u64(value: Option<i64>, field: &'static str) -> Result<Option<u64>, DownloadError> {
    value.map(|item| to_u64(item, field)).transpose()
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, DownloadError> {
    i64::try_from(value).map_err(|_| DownloadError::NumericOverflow(field))
}

pub(super) fn to_u64(value: i64, field: &'static str) -> Result<u64, DownloadError> {
    u64::try_from(value).map_err(|_| DownloadError::MissingObservationField(field))
}

fn to_u16(value: i64) -> Result<u16, DownloadError> {
    u16::try_from(value).map_err(|_| DownloadError::MissingObservationField("HTTP status"))
}
