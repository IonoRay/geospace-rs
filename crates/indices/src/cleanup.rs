//! Reference collection and bounded parsed-snapshot retention.

use std::{collections::HashSet, path::Path};

use ionoray_core::Sha256Digest;
use ionoray_store::{ScopedStore, StoreRoot, SyncGuard};

use crate::{
    IndexDataset, IndexError,
    database::{checkpoint, open_existing_year},
};

/// Prunes obsolete parsed rows and returns all raw bodies still referenced by
/// active/history snapshots or their selected sample origins. The caller must
/// hold the same dataset writer guard through subsequent raw-object cleanup.
pub(crate) async fn retain_snapshots(
    root: &StoreRoot,
    scoped: &ScopedStore,
    dataset: IndexDataset,
    _guard: &SyncGuard,
) -> Result<HashSet<Sha256Digest>, IndexError> {
    if scoped.scope().as_str() != dataset.scope_name() {
        return Err(IndexError::Validation {
            dataset,
            path: scoped.layout().object_catalog(),
            reason: "cleanup scope does not match dataset".to_owned(),
        });
    }
    let directory = root
        .layout()
        .index_database(dataset.provider(), dataset.as_str(), 0)
        .parent()
        .expect("year path has parent")
        .to_path_buf();
    let mut retained = HashSet::new();
    if !directory.exists() {
        return Ok(retained);
    }
    for entry in std::fs::read_dir(&directory).map_err(|source| io(&directory, source))? {
        let path = entry.map_err(|source| io(&directory, source))?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("db") {
            continue;
        }
        // Never follow a user-created partition symlink during cleanup.
        if !std::fs::symlink_metadata(&path)
            .map_err(|source| io(&path, source))?
            .file_type()
            .is_file()
        {
            return Err(IndexError::Validation {
                dataset,
                path,
                reason: "refusing non-regular year database during cleanup".to_owned(),
            });
        }
        let Some(year) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.parse::<u16>().ok())
        else {
            continue;
        };
        let database = open_existing_year(path, dataset, year).await?;
        let mut connection = database.database.connect()?;
        let transaction = connection.transaction().await?;
        let selected = "SELECT release_id FROM active_release UNION SELECT release_id FROM retained_history_release";
        let sources = format!(
            "{selected} UNION SELECT source_release_id FROM sample_provenance WHERE release_id IN ({selected})"
        );
        let mut rows = transaction.query(&format!("SELECT DISTINCT artifact_sha256 FROM dataset_release WHERE release_id IN ({sources})"), ()).await?;
        while let Some(row) = rows.next().await? {
            retained.insert(row.get::<String>(0)?.parse()?);
        }
        drop(rows);
        for table in tables(dataset)
            .iter()
            .copied()
            .chain(["release_sample_fingerprint", "sample_provenance"])
        {
            transaction
                .execute(
                    &format!("DELETE FROM {table} WHERE release_id NOT IN ({selected})"),
                    (),
                )
                .await?;
        }
        // Keep lightweight source/event metadata for provenance and auditing,
        // but make it impossible to reactivate a discarded data snapshot.
        transaction.execute(&format!("UPDATE dataset_release SET state = 'superseded' WHERE release_id NOT IN ({selected})"), ()).await?;
        transaction.commit().await?;
        checkpoint(&database.database, dataset, year).await?;
    }
    Ok(retained)
}

fn tables(dataset: IndexDataset) -> &'static [&'static str] {
    match dataset {
        IndexDataset::KpApF107 => &["geomagnetic_3h", "space_weather_daily"],
        IndexDataset::Dst => &["dst_hourly"],
        IndexDataset::Ae => &["ae_hourly"],
        IndexDataset::IriIgRz => &["iri_ig_rz_daily"],
        IndexDataset::IriApF107 => &["iri_f107_daily"],
    }
}

fn io(path: &Path, source: std::io::Error) -> IndexError {
    IndexError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
#[path = "cleanup/tests.rs"]
mod tests;
