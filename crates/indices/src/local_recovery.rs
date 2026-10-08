//! Recover accepted bodies first; packaged data is only a local fallback.
use ionoray_core::Sha256Digest;
use ionoray_store::{DownloadDisposition, DownloadOutcome, ScopedStore, StoreRoot};
use sha2::{Digest, Sha256};

use crate::{
    IndexDataset, IndexEdition, IndexError, IndexFileOrigin, ValidatedIndexFile,
    download::{IntervalFailure, IntervalFailureKind, download_cached_dataset_year_prepared},
    import::{ImportSummary, import_parsed_file},
    maintenance::open_dataset,
    parse::parse_file,
};

pub(crate) async fn recover(
    root: &StoreRoot,
    scoped: &ScopedStore,
    dataset: IndexDataset,
    start: i64,
    end: i64,
    allow_cache: bool,
) -> Result<
    (
        ImportSummary,
        Vec<IntervalFailure>,
        usize,
        Vec<ValidatedIndexFile>,
    ),
    IndexError,
> {
    let first = crate::time::epoch_from_millis(start).to_gregorian_utc().0;
    let last = crate::time::epoch_from_millis(end - 1).to_gregorian_utc().0;
    let mut total = ImportSummary::default();
    let mut files = Vec::new();
    let mut sources: std::collections::HashMap<String, (bool, bool)> =
        std::collections::HashMap::new();
    let mut failures = Vec::new();
    let mut cache_count = 0;
    for year in recovery_years(root, dataset, first, last)? {
        let artifacts = scoped
            .accepted_artifacts_for_year(dataset.provider(), dataset.as_str(), year)
            .await?;
        let mut recovered = false;
        for artifact in artifacts {
            if let Some(month) = artifact.month {
                let month_start = crate::parse::epoch_ms(year, month, 1, 0)
                    .map_err(|_| IndexError::InvalidNumber("month"))?;
                let month_end = if month == 12 {
                    crate::parse::epoch_ms(year + 1, 1, 1, 0)
                } else {
                    crate::parse::epoch_ms(year, month + 1, 1, 0)
                }
                .map_err(|_| IndexError::InvalidNumber("month"))?;
                if month_start >= end || month_end <= start {
                    continue;
                }
            }
            let source_status = sources
                .entry(artifact.source_id.clone())
                .or_insert((false, true));
            let result = recover_artifact(root, dataset, year, &artifact).await;
            match result {
                Ok((summary, file)) => {
                    source_status.0 |= summary.historical_revision;
                    add(&mut total, summary);
                    if let Some(file) = file {
                        files.push(file);
                    }
                    recovered = true;
                }
                Err(error) => {
                    source_status.1 = false;
                    failures.push(IntervalFailure::new(
                        year,
                        artifact.month,
                        IntervalFailureKind::Parse,
                        error.to_string(),
                    ));
                }
            }
        }
        if !recovered && allow_cache {
            recover_cache(
                root,
                dataset,
                year,
                &mut total,
                &mut files,
                &mut cache_count,
                &mut failures,
            )
            .await?;
        }
    }
    for (source, (revision, complete)) in sources {
        if complete {
            scoped.finalize_acceptance(&source, revision).await?;
        }
    }
    Ok((total, failures, cache_count, files))
}

fn recovery_years(
    root: &StoreRoot,
    dataset: IndexDataset,
    first: i32,
    last: i32,
) -> Result<Vec<u16>, IndexError> {
    let mut years = (first..=last)
        .map(|year| u16::try_from(year).map_err(|_| IndexError::InvalidNumber("recovery year")))
        .collect::<Result<Vec<_>, _>>()?;
    if !matches!(dataset, IndexDataset::Dst | IndexDataset::Ae) {
        let path = root
            .layout()
            .index_database(dataset.provider(), dataset.as_str(), 0);
        let directory = path.parent().expect("year database has parent");
        if directory.exists() {
            for entry in std::fs::read_dir(directory).map_err(|source| IndexError::Io {
                path: directory.to_owned(),
                source,
            })? {
                let entry = entry.map_err(|source| IndexError::Io {
                    path: directory.to_owned(),
                    source,
                })?;
                if let Some(year) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".db"))
                    .and_then(|stem| stem.parse::<u16>().ok())
                {
                    years.push(year);
                }
            }
        }
    }
    years.sort_unstable();
    years.dedup();
    Ok(years)
}

async fn recover_artifact(
    root: &StoreRoot,
    dataset: IndexDataset,
    year: u16,
    artifact: &ionoray_store::CatalogArtifact,
) -> Result<(ImportSummary, Option<ValidatedIndexFile>), IndexError> {
    let bytes = tokio::fs::read(&artifact.artifact.path)
        .await
        .map_err(|source| IndexError::ReadArtifact {
            path: artifact.artifact.path.clone(),
            source,
        })?;
    if Sha256Digest::from_bytes(Sha256::digest(&bytes).into()) != artifact.artifact.digest {
        return Err(IndexError::Validation {
            dataset,
            path: artifact.artifact.path.clone(),
            reason: "accepted local CAS digest mismatch".to_owned(),
        });
    }
    let database = open_dataset(root, dataset, year, false).await?;
    if crate::import::is_applied(&database, &artifact.source_id, artifact.artifact.digest).await? {
        return Ok((
            ImportSummary {
                releases_reused: 1,
                ..ImportSummary::default()
            },
            None,
        ));
    }
    let parsed = parse_file(dataset, &artifact.artifact.path, year, artifact.month).await?;
    let file = ValidatedIndexFile {
        source_id: artifact.source_id.clone(),
        canonical_url: None,
        dataset,
        year,
        month: artifact.month,
        edition: edition(&artifact.edition)?,
        records: 0,
        origin: IndexFileOrigin::LocalCas,
        download: DownloadOutcome {
            observation_id: "local-recovery".to_owned(),
            disposition: DownloadDisposition::Reused,
            queried_at_utc_ms: artifact.artifact.downloaded_at_utc_ms,
            finished_at_utc_ms: artifact.artifact.downloaded_at_utc_ms,
            artifact: artifact.artifact.clone(),
        },
    };
    let summary = import_parsed_file(&database, &file, parsed).await?;
    Ok((summary, Some(file)))
}

async fn recover_cache(
    root: &StoreRoot,
    dataset: IndexDataset,
    year: u16,
    total: &mut ImportSummary,
    files: &mut Vec<ValidatedIndexFile>,
    cache_count: &mut usize,
    failures: &mut Vec<IntervalFailure>,
) -> Result<(), IndexError> {
    match download_cached_dataset_year_prepared(root, dataset, year).await {
        Ok(report) => {
            for prepared in report.files {
                let database = open_dataset(root, dataset, year, false).await?;
                match import_parsed_file(&database, &prepared.file, prepared.parsed).await {
                    Ok(summary) => {
                        add(total, summary);
                        files.push(prepared.file);
                        *cache_count += 1;
                    }
                    Err(error) => failures.push(IntervalFailure::new(
                        year,
                        None,
                        IntervalFailureKind::Parse,
                        error.to_string(),
                    )),
                }
            }
        }
        Err(error) => failures.push(IntervalFailure::new(
            year,
            None,
            IntervalFailureKind::Unsupported,
            error.to_string(),
        )),
    }
    Ok(())
}

fn add(total: &mut ImportSummary, part: ImportSummary) {
    total.releases_created += part.releases_created;
    total.releases_reused += part.releases_reused;
    total.records += part.records;
    total.changes += part.changes;
    total.historical_revision |= part.historical_revision;
    total.initial += part.initial;
    total.append += part.append;
    total.backfill += part.backfill;
    total.revision += part.revision;
}
fn edition(value: &str) -> Result<IndexEdition, IndexError> {
    match value {
        "rolling" => Ok(IndexEdition::Rolling),
        "final" => Ok(IndexEdition::Final),
        "provisional" => Ok(IndexEdition::Provisional),
        "realtime" => Ok(IndexEdition::Realtime),
        _ => Err(IndexError::InvalidNumber("source edition")),
    }
}
