use crate::download::{
    DownloadError,
    repository::{DownloadRepository, PreviousArtifact, to_u64},
};

impl DownloadRepository<'_> {
    pub(crate) async fn latest_success(
        &self,
        source_id: &str,
    ) -> Result<Option<PreviousArtifact>, DownloadError> {
        self.previous(source_id, "SELECT f.artifact_sha256, a.byte_size, a.original_filename, a.media_type, f.last_modified_utc_ms, f.response_etag, f.last_modified_raw, f.final_url, a.downloaded_at_utc_ms, a.local_mtime_ns FROM fetch_attempt f JOIN artifact a ON a.sha256 = f.artifact_sha256 WHERE f.source_id = ? AND f.status IN ('downloaded', 'not_modified', 'metadata_unchanged', 'unchanged') ORDER BY f.queried_at_utc_ms DESC LIMIT 1").await
    }

    pub(crate) async fn latest_accepted(
        &self,
        source_id: &str,
    ) -> Result<Option<PreviousArtifact>, DownloadError> {
        self.previous(source_id, "SELECT a.sha256, a.byte_size, a.original_filename, a.media_type, f.last_modified_utc_ms, f.response_etag, f.last_modified_raw, f.final_url, a.downloaded_at_utc_ms, a.local_mtime_ns FROM accepted_source accepted JOIN artifact a ON a.sha256 = accepted.artifact_sha256 JOIN fetch_attempt f ON f.observation_id = accepted.observation_id WHERE accepted.source_id = ?").await
    }

    async fn previous(
        &self,
        source_id: &str,
        sql: &str,
    ) -> Result<Option<PreviousArtifact>, DownloadError> {
        let connection = self.catalog.connect()?;
        let mut rows = connection.query(sql, [source_id]).await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        let digest: String = row.get(0)?;
        Ok(Some(PreviousArtifact {
            digest: digest.parse()?,
            byte_size: to_u64(row.get(1)?, "artifact byte size")?,
            original_filename: row.get(2)?,
            media_type: row.get(3)?,
            source_modified_at_utc_ms: row.get(4)?,
            etag: row.get(5)?,
            last_modified_raw: row.get(6)?,
            final_url: row.get(7)?,
            downloaded_at_utc_ms: row.get(8)?,
            local_mtime_ns: row.get(9)?,
        }))
    }
}
