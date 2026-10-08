use uuid::Uuid;

use crate::{
    download::{
        ArtifactRef, DownloadDisposition, DownloadError, DownloadManager, DownloadOutcome,
        DownloadRequest,
    },
    time::unix_time_millis,
};

impl DownloadManager<'_> {
    /// Commits trusted package bytes through the same immutable CAS and
    /// provenance path as a remote response, without creating an HTTP request.
    ///
    /// # Errors
    /// Returns [`DownloadError`] for filesystem, integrity, or persistence
    /// failures.
    pub async fn ingest_bundled(
        &self,
        request: &DownloadRequest,
        bytes: &[u8],
        source_modified_at_utc_ms: i64,
    ) -> Result<DownloadOutcome, DownloadError> {
        let queried_at_utc_ms = unix_time_millis();
        let observation_id = Uuid::now_v7().as_hyphenated().to_string();
        self.repository
            .begin(
                &observation_id,
                request,
                "bundled_cache",
                queried_at_utc_ms,
                None,
            )
            .await?;
        let result = self
            .ingest_bundled_attempt(
                request,
                bytes,
                source_modified_at_utc_ms,
                &observation_id,
                queried_at_utc_ms,
            )
            .await;
        if let Err(error) = &result {
            let _ = self
                .repository
                .fail(
                    &observation_id,
                    "bundled_cache",
                    &error.to_string(),
                    unix_time_millis(),
                    None,
                )
                .await;
        }
        result
    }

    async fn ingest_bundled_attempt(
        &self,
        request: &DownloadRequest,
        bytes: &[u8],
        source_modified_at_utc_ms: i64,
        observation_id: &str,
        queried_at_utc_ms: i64,
    ) -> Result<DownloadOutcome, DownloadError> {
        self.repository
            .start_download(observation_id, queried_at_utc_ms)
            .await?;
        let object = self.cas.ingest_bytes(bytes).await?;
        let finished_at_utc_ms = unix_time_millis();
        let downloaded_at_utc_ms = self
            .repository
            .complete_bundled(
                observation_id,
                request,
                &object,
                source_modified_at_utc_ms,
                finished_at_utc_ms,
            )
            .await?;
        let disposition = if object.created {
            DownloadDisposition::Downloaded
        } else {
            DownloadDisposition::Reused
        };
        tracing::debug!(
            operation = "download.ingest_bundled",
            source_id = request.source_id(),
            artifact_sha256 = %object.digest,
            bytes = object.byte_size,
            "bundled bytes committed without network access"
        );
        Ok(DownloadOutcome {
            observation_id: observation_id.to_owned(),
            disposition,
            artifact: ArtifactRef {
                digest: object.digest,
                byte_size: object.byte_size,
                path: object.path,
                original_filename: request.logical_name().to_owned(),
                media_type: Some("text/plain".to_owned()),
                source_modified_at_utc_ms: Some(source_modified_at_utc_ms),
                downloaded_at_utc_ms,
                local_mtime_ns: object.local_mtime_ns,
            },
            queried_at_utc_ms,
            finished_at_utc_ms,
        })
    }
}
