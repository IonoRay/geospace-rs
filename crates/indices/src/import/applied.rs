use ionoray_core::Sha256Digest;
use turso::params;

use super::PARSER_VERSION;
use crate::{
    IndexError, ValidatedIndexFile,
    database::{YearDatabase, now_ms},
};

pub(crate) async fn is_applied(
    year_database: &YearDatabase,
    source_id: &str,
    artifact_sha256: Sha256Digest,
) -> Result<bool, IndexError> {
    let connection = year_database.database.connect()?;
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM applied_source applied JOIN dataset_release release ON release.release_id = applied.release_id WHERE applied.source_id = ? AND applied.artifact_sha256 = ? AND applied.parser_version = ? AND release.state = 'ready'",
            params![source_id, artifact_sha256.to_string(), PARSER_VERSION],
        )
        .await?;
    Ok(rows
        .next()
        .await?
        .ok_or(IndexError::InvalidNumber("applied source count"))?
        .get::<i64>(0)?
        != 0)
}

pub(super) async fn mark_applied(
    year_database: &YearDatabase,
    file: &ValidatedIndexFile,
    partition: &str,
    release_id: Option<&str>,
) -> Result<(), IndexError> {
    let mut connection = year_database.database.connect()?;
    let transaction = connection.transaction().await?;
    upsert_applied(&transaction, file, partition, release_id).await?;
    transaction.commit().await?;
    Ok(())
}

pub(super) async fn upsert_applied(
    transaction: &turso::transaction::Transaction<'_>,
    file: &ValidatedIndexFile,
    partition: &str,
    release_id: Option<&str>,
) -> Result<(), IndexError> {
    transaction.execute("INSERT INTO applied_source (source_id, partition_key, artifact_sha256, parser_version, release_id, applied_at_utc_ms) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(source_id, partition_key) DO UPDATE SET artifact_sha256=excluded.artifact_sha256, parser_version=excluded.parser_version, release_id=excluded.release_id, applied_at_utc_ms=excluded.applied_at_utc_ms", params![file.source_id.as_str(), partition, file.download.artifact.digest.to_string(), PARSER_VERSION, release_id, now_ms()]).await?;
    Ok(())
}
