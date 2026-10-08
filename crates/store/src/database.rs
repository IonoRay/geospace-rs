use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use ionoray_core::Sha256Digest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use turso::{Builder, Database};

use crate::{StoreError, time::unix_time_millis};

const SCHEMA_TABLE: &str = "
    CREATE TABLE _ionoray_schema (
        schema_id TEXT PRIMARY KEY,
        created_at_utc_ms INTEGER NOT NULL
    )
";

const OBJECT_CATALOG_SCHEMA: &[&str] = &[
    "CREATE TABLE artifact (sha256 TEXT PRIMARY KEY, byte_size INTEGER NOT NULL, relative_path TEXT NOT NULL, local_mtime_ns INTEGER NOT NULL, downloaded_at_utc_ms INTEGER NOT NULL, first_seen_at_utc_ms INTEGER NOT NULL, last_verified_at_utc_ms INTEGER NOT NULL, original_filename TEXT NOT NULL, media_type TEXT)",
    "CREATE TABLE source_object (source_id TEXT PRIMARY KEY, provider TEXT, dataset TEXT, year INTEGER, month INTEGER, edition TEXT, priority INTEGER, canonical_url TEXT NOT NULL, logical_name TEXT NOT NULL, created_at_utc_ms INTEGER NOT NULL, updated_at_utc_ms INTEGER NOT NULL)",
    "CREATE TABLE fetch_attempt (observation_id TEXT PRIMARY KEY, source_id TEXT NOT NULL, request_url TEXT NOT NULL, check_mode TEXT NOT NULL, queried_at_utc_ms INTEGER NOT NULL, download_started_at_utc_ms INTEGER, download_finished_at_utc_ms INTEGER, request_etag TEXT, request_last_modified TEXT, final_url TEXT, http_status INTEGER, response_etag TEXT, last_modified_raw TEXT, last_modified_utc_ms INTEGER, content_length_header INTEGER, bytes_received INTEGER, artifact_sha256 TEXT, status TEXT NOT NULL, error_kind TEXT, error_message TEXT, FOREIGN KEY (source_id) REFERENCES source_object(source_id), FOREIGN KEY (artifact_sha256) REFERENCES artifact(sha256))",
    "CREATE INDEX fetch_attempt_source_time ON fetch_attempt(source_id, queried_at_utc_ms DESC)",
    "CREATE TABLE artifact_origin (artifact_sha256 TEXT NOT NULL, source_id TEXT NOT NULL, original_filename TEXT NOT NULL, content_disposition TEXT, final_url TEXT NOT NULL, first_observation_id TEXT NOT NULL, last_observation_id TEXT NOT NULL, first_observed_at_utc_ms INTEGER NOT NULL, last_observed_at_utc_ms INTEGER NOT NULL, PRIMARY KEY (artifact_sha256, source_id), FOREIGN KEY (artifact_sha256) REFERENCES artifact(sha256), FOREIGN KEY (source_id) REFERENCES source_object(source_id))",
    "CREATE TABLE maintenance_run (run_id TEXT PRIMARY KEY, operation TEXT NOT NULL, policy TEXT NOT NULL, start_year INTEGER, end_year INTEGER, started_at_utc_ms INTEGER NOT NULL, finished_at_utc_ms INTEGER, status TEXT NOT NULL, sources_queried INTEGER NOT NULL DEFAULT 0, metadata_unchanged INTEGER NOT NULL DEFAULT 0, bodies_downloaded INTEGER NOT NULL DEFAULT 0, bytes_downloaded INTEGER NOT NULL DEFAULT 0, artifacts_created INTEGER NOT NULL DEFAULT 0, releases_created INTEGER NOT NULL DEFAULT 0, releases_reused INTEGER NOT NULL DEFAULT 0, error_kind TEXT, error_message TEXT)",
];

const ACCEPTED_SOURCE_TABLE: &str = "CREATE TABLE accepted_source (source_id TEXT PRIMARY KEY, artifact_sha256 TEXT NOT NULL, observation_id TEXT NOT NULL, pending_previous_sha256 TEXT, history_sha256 TEXT, accepted_at_utc_ms INTEGER NOT NULL, canonical_url TEXT NOT NULL, browse_relative TEXT NOT NULL, FOREIGN KEY (source_id) REFERENCES source_object(source_id), FOREIGN KEY (artifact_sha256) REFERENCES artifact(sha256), FOREIGN KEY (observation_id) REFERENCES fetch_attempt(observation_id))";
const ACCEPTED_VIEW_TABLE: &str = "CREATE TABLE accepted_view (browse_relative TEXT PRIMARY KEY, source_id TEXT NOT NULL, artifact_sha256 TEXT NOT NULL, accepted_at_utc_ms INTEGER NOT NULL)";

const SCOPED_OBJECT_CATALOG_SCHEMA: &[&str] = &[
    OBJECT_CATALOG_SCHEMA[0],
    OBJECT_CATALOG_SCHEMA[1],
    OBJECT_CATALOG_SCHEMA[2],
    OBJECT_CATALOG_SCHEMA[3],
    OBJECT_CATALOG_SCHEMA[4],
    OBJECT_CATALOG_SCHEMA[5],
    ACCEPTED_SOURCE_TABLE,
    ACCEPTED_VIEW_TABLE,
];

/// The role of an independently rebuilt Turso database.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DatabaseKind {
    /// Global CAS object and upstream-origin catalog.
    ObjectCatalog,
    /// Dataset-scoped object catalog with an explicit accepted baseline.
    ScopedObjectCatalog,
}

impl DatabaseKind {
    const fn statements(self) -> &'static [&'static str] {
        match self {
            Self::ObjectCatalog => OBJECT_CATALOG_SCHEMA,
            Self::ScopedObjectCatalog => SCOPED_OBJECT_CATALOG_SCHEMA,
        }
    }
}

/// Current schema identity for one Turso database.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatabaseStatus {
    /// Database role.
    pub kind: DatabaseKind,
    /// Exact schema identity compiled into this crate.
    pub schema_id: String,
    /// Whether this open replaced an absent or incompatible database.
    pub rebuilt: bool,
}

pub(crate) async fn open_database(
    path: &Path,
    kind: DatabaseKind,
) -> Result<(Database, DatabaseStatus), StoreError> {
    let schema_id = schema_id(kind);
    if path.exists() {
        let database = build_local(path).await?;
        if schema_matches(&database, &schema_id).await? {
            return Ok((database, status(kind, schema_id, false)));
        }
        drop(database);
        remove_database_files(path)?;
    }

    let database = build_local(path).await?;
    create_schema(&database, kind, &schema_id).await?;
    checkpoint(&database, "objects/catalog.db").await?;
    Ok((database, status(kind, schema_id, true)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CheckpointStatus {
    pub(crate) busy: i64,
    pub(crate) log_frames: i64,
    pub(crate) checkpointed_frames: i64,
}

#[tracing::instrument(
    name = "database.checkpoint",
    level = "debug",
    skip(database),
    fields(database = name),
    err(level = "warn")
)]
pub(crate) async fn checkpoint(
    database: &Database,
    name: &str,
) -> Result<CheckpointStatus, StoreError> {
    let started = Instant::now();
    let connection = database.connect()?;
    let mut rows = connection
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await?;
    let row = rows
        .next()
        .await?
        .ok_or(StoreError::MissingCheckpointStatus)?;
    let status = CheckpointStatus {
        busy: row.get(0)?,
        log_frames: row.get(1)?,
        checkpointed_frames: row.get(2)?,
    };
    while rows.next().await?.is_some() {}
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        operation = "database.checkpoint",
        database = name,
        busy = status.busy,
        log_frames = status.log_frames,
        checkpointed_frames = status.checkpointed_frames,
        elapsed_ms,
        "committed WAL frames flushed to the database file"
    );
    if status.busy != 0 {
        return Err(StoreError::CheckpointBusy {
            busy: status.busy,
            log_frames: status.log_frames,
            checkpointed_frames: status.checkpointed_frames,
        });
    }
    Ok(status)
}

fn status(kind: DatabaseKind, schema_id: String, rebuilt: bool) -> DatabaseStatus {
    DatabaseStatus {
        kind,
        schema_id,
        rebuilt,
    }
}

fn schema_id(kind: DatabaseKind) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ionoray-schema\0");
    hasher.update(match kind {
        DatabaseKind::ObjectCatalog => b"object-catalog" as &[u8],
        DatabaseKind::ScopedObjectCatalog => b"scoped-object-catalog" as &[u8],
    });
    hasher.update(SCHEMA_TABLE.as_bytes());
    for statement in kind.statements() {
        hasher.update(b"\0");
        hasher.update(statement.as_bytes());
    }
    Sha256Digest::from_bytes(hasher.finalize().into()).to_string()
}

async fn build_local(path: &Path) -> Result<Database, StoreError> {
    let path_text = path
        .to_str()
        .ok_or_else(|| StoreError::NonUtf8Path(path.to_path_buf()))?;
    Ok(Builder::new_local(path_text).build().await?)
}

async fn schema_matches(database: &Database, expected: &str) -> Result<bool, StoreError> {
    let connection = database.connect()?;
    let mut table = connection
        .query(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '_ionoray_schema'",
            (),
        )
        .await?;
    let count = table
        .next()
        .await?
        .ok_or(StoreError::MissingSchemaMetadata)?
        .get::<i64>(0)?;
    if count != 1 {
        return Ok(false);
    }

    let mut rows = connection
        .query("SELECT schema_id FROM _ionoray_schema LIMIT 1", ())
        .await?;
    Ok(rows
        .next()
        .await?
        .map(|row| row.get::<String>(0))
        .transpose()?
        .is_some_and(|value| value == expected))
}

async fn create_schema(
    database: &Database,
    kind: DatabaseKind,
    schema_id: &str,
) -> Result<(), StoreError> {
    let mut connection = database.connect()?;
    let transaction = connection.transaction().await?;
    transaction.execute(SCHEMA_TABLE, ()).await?;
    for statement in kind.statements() {
        transaction.execute(statement, ()).await?;
    }
    transaction
        .execute(
            "INSERT INTO _ionoray_schema (schema_id, created_at_utc_ms) VALUES (?, ?)",
            (schema_id, unix_time_millis()),
        )
        .await?;
    transaction.commit().await?;
    Ok(())
}

fn remove_database_files(path: &Path) -> Result<(), StoreError> {
    for candidate in database_files(path) {
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(StoreError::io(candidate, error)),
        }
    }
    Ok(())
}

fn database_files(path: &Path) -> [PathBuf; 3] {
    let text = path.as_os_str().to_string_lossy();
    [
        path.to_path_buf(),
        PathBuf::from(format!("{text}-wal")),
        PathBuf::from(format!("{text}-shm")),
    ]
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn reuses_current_schema_without_rebuilding() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("catalog.db");

        let (database, first) = open_database(&path, DatabaseKind::ObjectCatalog)
            .await
            .unwrap();
        assert!(first.rebuilt);
        drop(database);

        let (_, second) = open_database(&path, DatabaseKind::ObjectCatalog)
            .await
            .unwrap();
        assert!(!second.rebuilt);
        assert!(second.schema_id.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn replaces_incompatible_schema_instead_of_migrating() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("catalog.db");
        let database = build_local(&path).await.unwrap();
        let connection = database.connect().unwrap();
        connection
            .execute("CREATE TABLE legacy_only (value TEXT)", ())
            .await
            .unwrap();
        drop(connection);
        drop(database);

        let (database, status) = open_database(&path, DatabaseKind::ObjectCatalog)
            .await
            .unwrap();
        assert!(status.rebuilt);
        let connection = database.connect().unwrap();
        let mut rows = connection
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'legacy_only'",
                (),
            )
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }
}
