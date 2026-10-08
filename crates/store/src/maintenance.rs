use serde::{Deserialize, Serialize};
use turso::{Database, params};
use uuid::Uuid;

use crate::{StoreError, database::checkpoint, time::unix_time_millis};

/// Aggregate counters persisted for one maintenance operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceSummary {
    /// Remote source objects queried.
    pub sources_queried: usize,
    /// Metadata comparisons that skipped a body.
    pub metadata_unchanged: usize,
    /// Bodies transferred.
    pub bodies_downloaded: usize,
    /// Transferred bytes.
    pub bytes_downloaded: u64,
    /// New content-addressed objects.
    pub artifacts_created: usize,
    /// New parsed releases.
    pub releases_created: usize,
    /// Existing parsed releases reused.
    pub releases_reused: usize,
}

/// Persisted identity of one running maintenance operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceRun {
    /// `UUIDv7` operation identity.
    pub run_id: String,
    /// UTC start timestamp.
    pub started_at_utc_ms: i64,
}

pub(crate) async fn begin(
    database: &Database,
    operation: &str,
    policy: &str,
    year: Option<u16>,
) -> Result<MaintenanceRun, StoreError> {
    let run = MaintenanceRun {
        run_id: Uuid::now_v7().as_hyphenated().to_string(),
        started_at_utc_ms: unix_time_millis(),
    };
    database
        .connect()?
        .execute(
            "INSERT INTO maintenance_run (run_id, operation, policy, start_year, end_year, started_at_utc_ms, status) VALUES (?, ?, ?, ?, ?, ?, 'running')",
            params![run.run_id.as_str(), operation, policy, year.map(i64::from), year.map(i64::from), run.started_at_utc_ms],
        )
        .await?;
    checkpoint(database, "objects/catalog.db").await?;
    Ok(run)
}

pub(crate) async fn interrupt_stale(database: &Database) -> Result<(), StoreError> {
    let now = unix_time_millis();
    let cutoff = now.saturating_sub(6 * 60 * 60 * 1_000);
    database
        .connect()?
        .execute(
            "UPDATE maintenance_run SET finished_at_utc_ms = ?, status = 'interrupted', error_kind = 'interrupted', error_message = 'stale operation recovered during initialization' WHERE status = 'running' AND started_at_utc_ms < ?",
            params![now, cutoff],
        )
        .await?;
    checkpoint(database, "objects/catalog.db").await?;
    Ok(())
}

pub(crate) async fn finish(
    database: &Database,
    run: &MaintenanceRun,
    summary: MaintenanceSummary,
) -> Result<(), StoreError> {
    database
        .connect()?
        .execute(
            "UPDATE maintenance_run SET finished_at_utc_ms = ?, status = 'completed', sources_queried = ?, metadata_unchanged = ?, bodies_downloaded = ?, bytes_downloaded = ?, artifacts_created = ?, releases_created = ?, releases_reused = ? WHERE run_id = ?",
            params![unix_time_millis(), to_i64(summary.sources_queried)?, to_i64(summary.metadata_unchanged)?, to_i64(summary.bodies_downloaded)?, i64::try_from(summary.bytes_downloaded).map_err(|_| StoreError::NumericOverflow)?, to_i64(summary.artifacts_created)?, to_i64(summary.releases_created)?, to_i64(summary.releases_reused)?, run.run_id.as_str()],
        )
        .await?;
    checkpoint(database, "objects/catalog.db").await?;
    Ok(())
}

pub(crate) async fn fail(
    database: &Database,
    run: &MaintenanceRun,
    kind: &str,
    message: &str,
) -> Result<(), StoreError> {
    database
        .connect()?
        .execute(
            "UPDATE maintenance_run SET finished_at_utc_ms = ?, status = 'failed', error_kind = ?, error_message = ? WHERE run_id = ?",
            params![unix_time_millis(), kind, message, run.run_id.as_str()],
        )
        .await?;
    checkpoint(database, "objects/catalog.db").await?;
    Ok(())
}

fn to_i64(value: usize) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericOverflow)
}
