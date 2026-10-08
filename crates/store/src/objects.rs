use ionoray_core::Sha256Digest;
use serde::{Deserialize, Serialize};
use turso::Database;

use crate::{ArtifactRef, DownloadError, StoreLayout};

/// One upstream identity associated with a content-addressed object.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactOrigin {
    /// Stable source-object identity.
    pub source_id: String,
    /// Requested canonical URL.
    pub canonical_url: String,
    /// Actual final URL after redirects.
    pub final_url: String,
    /// Portable logical filename selected by the dataset package.
    pub original_filename: String,
    /// Raw upstream `Content-Disposition`, when supplied.
    pub content_disposition: Option<String>,
    /// First successful observation time.
    pub first_observed_at_utc_ms: i64,
    /// Most recent successful observation time.
    pub last_observed_at_utc_ms: i64,
}

/// One CAS artifact associated with an upstream dataset partition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CatalogArtifact {
    /// Stable source-object identity.
    pub source_id: String,
    /// Dataset provider.
    pub provider: String,
    /// Dataset name.
    pub dataset: String,
    /// Calendar year partition.
    pub year: u16,
    /// Optional monthly partition.
    pub month: Option<u8>,
    /// Publication edition.
    pub edition: String,
    /// Candidate priority.
    pub priority: i32,
    /// Actual final URL after redirects.
    pub final_url: String,
    /// Raw upstream `Content-Disposition`, when supplied.
    pub content_disposition: Option<String>,
    /// Local artifact and remote timestamps.
    pub artifact: ArtifactRef,
}

pub(crate) async fn artifacts_for_year(
    database: &Database,
    layout: &StoreLayout,
    provider: &str,
    dataset: &str,
    year: u16,
) -> Result<Vec<CatalogArtifact>, DownloadError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query(
            "SELECT s.source_id, s.provider, s.dataset, s.year, s.month, s.edition, s.priority, a.sha256, a.byte_size, a.original_filename, a.media_type, f.last_modified_utc_ms, a.downloaded_at_utc_ms, a.local_mtime_ns, o.final_url, o.content_disposition FROM source_object s JOIN artifact_origin o ON o.source_id = s.source_id JOIN artifact a ON a.sha256 = o.artifact_sha256 JOIN fetch_attempt f ON f.observation_id = o.last_observation_id WHERE s.provider = ? AND s.dataset = ? AND s.year = ? ORDER BY s.month, s.priority DESC, o.last_observed_at_utc_ms DESC",
            turso::params![provider, dataset, i64::from(year)],
        )
        .await?;
    let mut artifacts = Vec::new();
    while let Some(row) = rows.next().await? {
        let digest: Sha256Digest = row.get::<String>(7)?.parse()?;
        artifacts.push(CatalogArtifact {
            source_id: row.get(0)?,
            provider: row.get::<Option<String>>(1)?.unwrap_or_default(),
            dataset: row.get::<Option<String>>(2)?.unwrap_or_default(),
            year: u16::try_from(row.get::<Option<i64>>(3)?.unwrap_or_default())
                .map_err(|_| DownloadError::NumericOverflow("source year"))?,
            month: row
                .get::<Option<i64>>(4)?
                .map(|value| {
                    u8::try_from(value).map_err(|_| DownloadError::NumericOverflow("source month"))
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
                path: layout.object_path(digest),
                original_filename: row.get(9)?,
                media_type: row.get(10)?,
                source_modified_at_utc_ms: row.get(11)?,
                downloaded_at_utc_ms: row.get(12)?,
                local_mtime_ns: row.get(13)?,
            },
        });
    }
    Ok(artifacts)
}

pub(crate) async fn origins(
    database: &Database,
    digest: Sha256Digest,
) -> Result<Vec<ArtifactOrigin>, DownloadError> {
    let connection = database.connect()?;
    let digest = digest.to_string();
    let mut rows = connection
        .query(
            "SELECT o.source_id, s.canonical_url, o.final_url, o.original_filename, o.content_disposition, o.first_observed_at_utc_ms, o.last_observed_at_utc_ms FROM artifact_origin o JOIN source_object s ON s.source_id = o.source_id WHERE o.artifact_sha256 = ? ORDER BY o.first_observed_at_utc_ms, o.source_id",
            [digest.as_str()],
        )
        .await?;
    let mut origins = Vec::new();
    while let Some(row) = rows.next().await? {
        origins.push(ArtifactOrigin {
            source_id: row.get(0)?,
            canonical_url: row.get(1)?,
            final_url: row.get(2)?,
            original_filename: row.get(3)?,
            content_disposition: row.get(4)?,
            first_observed_at_utc_ms: row.get(5)?,
            last_observed_at_utc_ms: row.get(6)?,
        });
    }
    Ok(origins)
}
