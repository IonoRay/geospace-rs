use crate::{
    ArtifactRef, CatalogArtifact, DownloadError, DownloadManager, ScopedStore,
    objects::artifacts_for_year,
};
use ionoray_core::Sha256Digest;

impl ScopedStore {
    /// Downloads candidates using only domain-accepted artifacts as HTTP baselines.
    ///
    /// # Errors
    /// Returns an error when the HTTP client cannot be built.
    pub fn downloads(&self) -> Result<DownloadManager<'_>, DownloadError> {
        let mut manager = DownloadManager::new(&self.catalog, &self.layout)?;
        manager.repository.use_accepted_baseline();
        Ok(manager)
    }
    /// Returns downloaded artifacts for local recovery before domain validation.
    ///
    /// # Errors
    /// Returns an error when catalog rows cannot be read.
    pub async fn artifacts_for_year(
        &self,
        provider: &str,
        dataset: &str,
        year: u16,
    ) -> Result<Vec<CatalogArtifact>, DownloadError> {
        artifacts_for_year(&self.catalog, &self.layout, provider, dataset, year).await
    }
    /// Returns only domain-accepted raw artifacts usable for offline recovery.
    ///
    /// Rolling sources (`month IS NULL`) are returned for every requested year.
    ///
    /// # Errors
    /// Returns an error when accepted catalog rows cannot be read.
    pub async fn accepted_artifacts_for_year(
        &self,
        provider: &str,
        dataset: &str,
        year: u16,
    ) -> Result<Vec<CatalogArtifact>, DownloadError> {
        let connection = self.catalog.connect()?;
        let mut rows = connection.query("SELECT s.source_id,s.provider,s.dataset,s.year,s.month,s.edition,s.priority,a.sha256,a.byte_size,a.original_filename,a.media_type,f.last_modified_utc_ms,a.downloaded_at_utc_ms,a.local_mtime_ns,f.final_url,o.content_disposition FROM accepted_source x JOIN source_object s ON s.source_id=x.source_id JOIN artifact a ON a.sha256=x.artifact_sha256 JOIN fetch_attempt f ON f.observation_id=x.observation_id LEFT JOIN artifact_origin o ON o.artifact_sha256=a.sha256 AND o.source_id=s.source_id WHERE s.provider=? AND s.dataset=? AND (s.year=? OR s.month IS NULL) ORDER BY s.priority DESC,s.month", turso::params![provider,dataset,i64::from(year)]).await?;
        let mut result = Vec::new();
        while let Some(row) = rows.next().await? {
            let digest: Sha256Digest = row.get::<String>(7)?.parse()?;
            result.push(CatalogArtifact {
                source_id: row.get(0)?,
                provider: row.get::<Option<String>>(1)?.unwrap_or_default(),
                dataset: row.get::<Option<String>>(2)?.unwrap_or_default(),
                year: if row.get::<Option<i64>>(4)?.is_none() {
                    year
                } else {
                    u16::try_from(row.get::<Option<i64>>(3)?.unwrap_or_default())
                        .map_err(|_| DownloadError::NumericOverflow("source year"))?
                },
                month: row
                    .get::<Option<i64>>(4)?
                    .map(|value| {
                        u8::try_from(value)
                            .map_err(|_| DownloadError::NumericOverflow("source month"))
                    })
                    .transpose()?,
                edition: row.get::<Option<String>>(5)?.unwrap_or_default(),
                priority: i32::try_from(row.get::<Option<i64>>(6)?.unwrap_or_default())
                    .map_err(|_| DownloadError::NumericOverflow("source priority"))?,
                final_url: row.get(14)?,
                content_disposition: row.get(15)?,
                artifact: ArtifactRef {
                    digest,
                    byte_size: u64::try_from(row.get::<i64>(8)?)
                        .map_err(|_| DownloadError::NumericOverflow("artifact size"))?,
                    path: self.layout.object_path(digest),
                    original_filename: row.get(9)?,
                    media_type: row.get(10)?,
                    source_modified_at_utc_ms: row.get(11)?,
                    downloaded_at_utc_ms: row.get(12)?,
                    local_mtime_ns: row.get(13)?,
                },
            });
        }
        Ok(result)
    }
}
