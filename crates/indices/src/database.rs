use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use ionoray_core::Sha256Digest;
use sha2::{Digest, Sha256};
use turso::{Builder, Database};

use crate::{IndexDataset, IndexError};

const SCHEMA_TABLE: &str = "CREATE TABLE _ionoray_schema (schema_id TEXT PRIMARY KEY, provider TEXT NOT NULL, dataset TEXT NOT NULL, year INTEGER NOT NULL, created_at_utc_ms INTEGER NOT NULL)";
const RELEASE_TABLE: &str = "CREATE TABLE dataset_release (release_id TEXT PRIMARY KEY, artifact_sha256 TEXT NOT NULL, source_id TEXT NOT NULL, edition TEXT NOT NULL, edition_priority INTEGER NOT NULL, parser_name TEXT NOT NULL, parser_version TEXT NOT NULL, coverage_start_utc_ms INTEGER NOT NULL, coverage_end_utc_ms INTEGER NOT NULL, source_modified_at_utc_ms INTEGER, fetched_at_utc_ms INTEGER NOT NULL, imported_at_utc_ms INTEGER NOT NULL, record_count INTEGER NOT NULL, state TEXT NOT NULL)";
const RELEASE_LOOKUP: &str = "CREATE INDEX dataset_release_latest ON dataset_release(state, edition_priority DESC, source_modified_at_utc_ms DESC, fetched_at_utc_ms DESC, imported_at_utc_ms DESC)";
const ACTIVE_RELEASE_TABLE: &str = "CREATE TABLE active_release (partition_key TEXT PRIMARY KEY, release_id TEXT NOT NULL REFERENCES dataset_release(release_id))";
const HISTORY_RELEASE_TABLE: &str = "CREATE TABLE retained_history_release (partition_key TEXT PRIMARY KEY, release_id TEXT NOT NULL REFERENCES dataset_release(release_id))";
const SAMPLE_FINGERPRINT_TABLE: &str = "CREATE TABLE release_sample_fingerprint (release_id TEXT NOT NULL REFERENCES dataset_release(release_id), sample_key TEXT NOT NULL, epoch_utc_ms INTEGER NOT NULL, semantic_sha256 TEXT NOT NULL, value_json TEXT NOT NULL, PRIMARY KEY (release_id, sample_key))";
const SAMPLE_FINGERPRINT_LOOKUP: &str = "CREATE INDEX release_sample_fingerprint_active ON release_sample_fingerprint(release_id, epoch_utc_ms)";
const SAMPLE_PROVENANCE_TABLE: &str = "CREATE TABLE sample_provenance (release_id TEXT NOT NULL REFERENCES dataset_release(release_id), sample_key TEXT NOT NULL, source_release_id TEXT NOT NULL REFERENCES dataset_release(release_id), PRIMARY KEY (release_id, sample_key))";
const APPLIED_SOURCE_TABLE: &str = "CREATE TABLE applied_source (source_id TEXT NOT NULL, partition_key TEXT NOT NULL, artifact_sha256 TEXT NOT NULL, parser_version TEXT NOT NULL, release_id TEXT REFERENCES dataset_release(release_id), applied_at_utc_ms INTEGER NOT NULL, PRIMARY KEY (source_id, partition_key))";
const CHANGE_TABLE: &str = "CREATE TABLE release_change (change_id INTEGER PRIMARY KEY, previous_release_id TEXT REFERENCES dataset_release(release_id), active_release_id TEXT NOT NULL REFERENCES dataset_release(release_id), change_kind TEXT NOT NULL, added_count INTEGER NOT NULL, modified_count INTEGER NOT NULL, removed_count INTEGER NOT NULL, changed_start_utc_ms INTEGER, changed_end_utc_ms INTEGER, committed_at_utc_ms INTEGER NOT NULL)";
const FIELD_CHANGE_TABLE: &str = "CREATE TABLE release_field_change (change_id INTEGER NOT NULL REFERENCES release_change(change_id), sample_key TEXT NOT NULL, field_name TEXT NOT NULL, old_value_json TEXT, new_value_json TEXT, old_semantic_sha256 TEXT, new_semantic_sha256 TEXT, change_kind TEXT NOT NULL, PRIMARY KEY (change_id, sample_key, field_name))";

const GFZ_SCHEMA: &[&str] = &[
    "CREATE TABLE geomagnetic_3h (release_id TEXT NOT NULL, interval_start_utc_ms INTEGER NOT NULL, kp_thirds INTEGER, ap REAL, PRIMARY KEY (release_id, interval_start_utc_ms))",
    "CREATE INDEX geomagnetic_3h_time ON geomagnetic_3h(interval_start_utc_ms)",
    "CREATE TABLE space_weather_daily (release_id TEXT NOT NULL, date_utc_ms INTEGER NOT NULL, ap_daily REAL, sunspot_number REAL, f107_observed_sfu REAL, f107_observed_interpolation_gap_days INTEGER NOT NULL, f107_adjusted_sfu REAL, f107a_81d_sfu REAL, f107a_81d_interpolated_input_count INTEGER NOT NULL, quality TEXT NOT NULL, PRIMARY KEY (release_id, date_utc_ms))",
    "CREATE INDEX space_weather_daily_time ON space_weather_daily(date_utc_ms)",
];
const DST_SCHEMA: &[&str] = &[
    "CREATE TABLE dst_hourly (release_id TEXT NOT NULL, epoch_utc_ms INTEGER NOT NULL, dst_nt REAL, quality TEXT NOT NULL, PRIMARY KEY (release_id, epoch_utc_ms))",
    "CREATE INDEX dst_hourly_time ON dst_hourly(epoch_utc_ms)",
];
const AE_SCHEMA: &[&str] = &[
    "CREATE TABLE ae_hourly (release_id TEXT NOT NULL, epoch_utc_ms INTEGER NOT NULL, ae_nt REAL, quality TEXT NOT NULL, PRIMARY KEY (release_id, epoch_utc_ms))",
    "CREATE INDEX ae_hourly_time ON ae_hourly(epoch_utc_ms)",
];
const IRI_IG_RZ_SCHEMA: &[&str] = &[
    "CREATE TABLE iri_ig_rz_daily (release_id TEXT NOT NULL, date_utc_ms INTEGER NOT NULL, ig12 REAL NOT NULL, rz12 REAL NOT NULL, quality TEXT NOT NULL, PRIMARY KEY (release_id, date_utc_ms))",
    "CREATE INDEX iri_ig_rz_daily_time ON iri_ig_rz_daily(date_utc_ms)",
];
const IRI_APF107_SCHEMA: &[&str] = &[
    "CREATE TABLE iri_f107_daily (release_id TEXT NOT NULL, date_utc_ms INTEGER NOT NULL, f107_adjusted_sfu REAL NOT NULL, f107a_81d_adjusted_sfu REAL NOT NULL, f107a_365d_adjusted_sfu REAL NOT NULL, PRIMARY KEY (release_id, date_utc_ms))",
    "CREATE INDEX iri_f107_daily_time ON iri_f107_daily(date_utc_ms)",
];

pub(crate) struct YearDatabase {
    pub(crate) database: Database,
}

pub(crate) async fn open_year(
    path: PathBuf,
    dataset: IndexDataset,
    year: u16,
) -> Result<YearDatabase, IndexError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| IndexError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let expected = schema_id(dataset);
    if path.exists() {
        let database = build(&path).await?;
        if schema_matches(&database, &expected).await? {
            return Ok(YearDatabase { database });
        }
        drop(database);
        remove_database_files(&path)?;
    }
    let database = build(&path).await?;
    create_schema(&database, dataset, year, &expected).await?;
    checkpoint(&database, dataset, year).await?;
    Ok(YearDatabase { database })
}

pub(crate) async fn open_existing_year(
    path: PathBuf,
    dataset: IndexDataset,
    year: u16,
) -> Result<YearDatabase, IndexError> {
    if !path.exists() {
        return Err(IndexError::MissingYearData { dataset, year });
    }
    open_year(path, dataset, year).await
}

fn statements(dataset: IndexDataset) -> &'static [&'static str] {
    match dataset {
        IndexDataset::KpApF107 => GFZ_SCHEMA,
        IndexDataset::Dst => DST_SCHEMA,
        IndexDataset::Ae => AE_SCHEMA,
        IndexDataset::IriIgRz => IRI_IG_RZ_SCHEMA,
        IndexDataset::IriApF107 => IRI_APF107_SCHEMA,
    }
}

fn schema_id(dataset: IndexDataset) -> String {
    let mut hasher = Sha256::new();
    for statement in [
        SCHEMA_TABLE,
        RELEASE_TABLE,
        RELEASE_LOOKUP,
        ACTIVE_RELEASE_TABLE,
        HISTORY_RELEASE_TABLE,
        SAMPLE_FINGERPRINT_TABLE,
        SAMPLE_FINGERPRINT_LOOKUP,
        SAMPLE_PROVENANCE_TABLE,
        APPLIED_SOURCE_TABLE,
        CHANGE_TABLE,
        FIELD_CHANGE_TABLE,
    ]
    .into_iter()
    .chain(statements(dataset).iter().copied())
    {
        hasher.update(statement.as_bytes());
        hasher.update(b"\0");
    }
    Sha256Digest::from_bytes(hasher.finalize().into()).to_string()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointStatus {
    pub(crate) busy: i64,
    pub(crate) log_frames: i64,
    pub(crate) checkpointed_frames: i64,
}

#[tracing::instrument(
    name = "indices.database.checkpoint",
    level = "debug",
    skip(database),
    fields(dataset = ?dataset, year),
    err(level = "warn")
)]
pub(crate) async fn checkpoint(
    database: &Database,
    dataset: IndexDataset,
    year: u16,
) -> Result<CheckpointStatus, IndexError> {
    let started = Instant::now();
    let connection = database.connect()?;
    let mut rows = connection
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or(IndexError::MissingCheckpointStatus)?;
    let status = CheckpointStatus {
        busy: row.get(0)?,
        log_frames: row.get(1)?,
        checkpointed_frames: row.get(2)?,
    };
    while rows.next().await?.is_some() {}
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        operation = "indices.database.checkpoint",
        dataset = ?dataset,
        year,
        busy = status.busy,
        log_frames = status.log_frames,
        checkpointed_frames = status.checkpointed_frames,
        elapsed_ms,
        "yearly WAL frames flushed to the database file"
    );
    if status.busy != 0 {
        return Err(IndexError::CheckpointBusy {
            busy: status.busy,
            log_frames: status.log_frames,
            checkpointed_frames: status.checkpointed_frames,
        });
    }
    Ok(status)
}

async fn build(path: &Path) -> Result<Database, IndexError> {
    let text = path.to_str().ok_or_else(|| IndexError::Validation {
        dataset: IndexDataset::KpApF107,
        path: path.to_path_buf(),
        reason: "database path is not UTF-8".to_owned(),
    })?;
    Ok(Builder::new_local(text).build().await?)
}

async fn schema_matches(database: &Database, expected: &str) -> Result<bool, IndexError> {
    let connection = database.connect()?;
    let mut rows = connection
        .query("SELECT schema_id FROM _ionoray_schema LIMIT 1", ())
        .await;
    let Ok(ref mut rows) = rows else {
        return Ok(false);
    };
    Ok(rows
        .next()
        .await?
        .map(|row| row.get::<String>(0))
        .transpose()?
        .is_some_and(|value| value == expected))
}

async fn create_schema(
    database: &Database,
    dataset: IndexDataset,
    year: u16,
    schema_id: &str,
) -> Result<(), IndexError> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction().await?;
    for statement in [
        SCHEMA_TABLE,
        RELEASE_TABLE,
        RELEASE_LOOKUP,
        ACTIVE_RELEASE_TABLE,
        HISTORY_RELEASE_TABLE,
        SAMPLE_FINGERPRINT_TABLE,
        SAMPLE_FINGERPRINT_LOOKUP,
        SAMPLE_PROVENANCE_TABLE,
        APPLIED_SOURCE_TABLE,
        CHANGE_TABLE,
        FIELD_CHANGE_TABLE,
    ]
    .into_iter()
    .chain(statements(dataset).iter().copied())
    {
        transaction.execute(statement, ()).await?;
    }
    transaction
        .execute(
            "INSERT INTO _ionoray_schema (schema_id, provider, dataset, year, created_at_utc_ms) VALUES (?, ?, ?, ?, ?)",
            turso::params![schema_id, dataset.provider(), dataset.as_str(), i64::from(year), now_ms()],
        )
        .await?;
    transaction.commit().await?;
    Ok(())
}

fn remove_database_files(path: &Path) -> Result<(), IndexError> {
    let text = path.as_os_str().to_string_lossy();
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{text}-wal")),
        PathBuf::from(format!("{text}-shm")),
    ] {
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(IndexError::Io {
                    path: candidate,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn creates_and_reuses_one_year_database() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("gfz/2020.db");
        let first = open_year(path.clone(), IndexDataset::KpApF107, 2020)
            .await
            .unwrap();
        drop(first);
        let second = open_year(path, IndexDataset::KpApF107, 2020).await.unwrap();
        let connection = second.database.connect().unwrap();
        let mut rows = connection
            .query("SELECT COUNT(*) FROM _ionoray_schema", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            1
        );
    }
}
