use crate::maintenance::{datasets, open_dataset, required_partitions, scoped};
use crate::{DatasetVerifyReport, IndexDataset, IndexError, VerifyReport, database::YearDatabase};
use ionoray_core::Sha256Digest;
use ionoray_store::StoreRoot;
use sha2::{Digest, Sha256};

#[tracing::instrument(
    name = "indices.verify_year",
    level = "info",
    skip(store),
    fields(year),
    err(level = "warn")
)]
pub(crate) async fn verify_year(store: &StoreRoot, year: u16) -> Result<VerifyReport, IndexError> {
    let mut artifacts_verified = 0;
    let mut datasets_report = Vec::new();
    for dataset in datasets() {
        let scoped = scoped(store, dataset).await?;
        let _guard = scoped.try_acquire_read_guard()?;
        let database = open_dataset(store, dataset, year, true).await?;
        let ready = ready_partitions(&database).await?;
        let required = required_partitions(dataset);
        datasets_report.push(DatasetVerifyReport {
            dataset,
            ready_partitions: ready,
            required_partitions: required,
        });
        let connection = database.database.connect()?;
        let selected = "SELECT release_id FROM active_release UNION SELECT release_id FROM retained_history_release";
        let mut rows = connection.query(&format!("SELECT DISTINCT artifact_sha256 FROM dataset_release WHERE release_id IN ({selected} UNION SELECT source_release_id FROM sample_provenance WHERE release_id IN ({selected}))"), ()).await?;
        while let Some(row) = rows.next().await? {
            let digest: Sha256Digest = row.get::<String>(0)?.parse()?;
            verify_artifact(dataset, &scoped.layout().object_path(digest), digest).await?;
            artifacts_verified += 1;
        }
        if !dataset_year_is_ready(store, dataset, year).await? {
            return Err(IndexError::Validation {
                dataset,
                path: store
                    .layout()
                    .index_database(dataset.provider(), dataset.as_str(), year),
                reason: "requested year has missing active field values".to_owned(),
            });
        }
    }
    if let Some(incomplete) = datasets_report
        .iter()
        .find(|report| report.ready_partitions < report.required_partitions)
    {
        return Err(IndexError::Validation {
            dataset: incomplete.dataset,
            path: store.layout().root().to_path_buf(),
            reason: format!("incomplete ready partitions: {datasets_report:?}"),
        });
    }
    Ok(VerifyReport {
        year,
        artifacts_verified,
        datasets: datasets_report,
    })
}

pub(crate) async fn year_is_ready(store: &StoreRoot, year: u16) -> Result<bool, IndexError> {
    for dataset in datasets() {
        if !dataset_year_is_ready(store, dataset, year).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) async fn dataset_year_is_ready(
    store: &StoreRoot,
    dataset: IndexDataset,
    year: u16,
) -> Result<bool, IndexError> {
    let path = store
        .layout()
        .index_database(dataset.provider(), dataset.as_str(), year);
    if !path.exists() {
        return Ok(false);
    }
    let scoped = scoped(store, dataset).await?;
    let _guard = scoped.try_acquire_read_guard()?;
    let start = crate::parse::epoch_ms(year, 1, 1, 0)
        .map_err(|_| IndexError::InvalidNumber("year start"))?;
    let end = crate::parse::epoch_ms(
        year.checked_add(1)
            .ok_or(IndexError::InvalidNumber("year end"))?,
        1,
        1,
        0,
    )
    .map_err(|_| IndexError::InvalidNumber("year end"))?;
    let coverage = crate::coverage::audit(
        store,
        dataset,
        crate::IndexField::for_dataset(dataset),
        start,
        end,
        crate::CoverageGapReason::LocalMissing,
    )
    .await?;
    Ok(coverage.iter().all(|field| field.gaps.is_empty()))
}

async fn ready_partitions(database: &YearDatabase) -> Result<usize, IndexError> {
    let connection = database.database.connect()?;
    let mut rows = connection
        .query("SELECT COUNT(*) FROM active_release", ())
        .await?;
    let count = rows.next().await?.map_or(Ok(0), |row| row.get::<i64>(0))?;
    usize::try_from(count).map_err(|_| IndexError::InvalidNumber("ready partitions"))
}

async fn verify_artifact(
    dataset: IndexDataset,
    path: &std::path::Path,
    expected: Sha256Digest,
) -> Result<(), IndexError> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|source| IndexError::ReadArtifact {
            path: path.to_path_buf(),
            source,
        })?;
    let actual = Sha256Digest::from_bytes(Sha256::digest(bytes).into());
    if actual != expected {
        return Err(IndexError::Validation {
            dataset,
            path: path.to_path_buf(),
            reason: format!("CAS digest is {actual}, expected {expected}"),
        });
    }
    Ok(())
}
