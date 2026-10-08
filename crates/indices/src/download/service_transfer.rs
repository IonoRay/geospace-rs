//! Candidate transfer and acceptance helpers for index downloads.

use ionoray_store::{CheckMode, ScopedStore, StoreRoot};
use sha2::{Digest, Sha256};

use super::{
    IntervalFailure, PreparedIndexFile, PreparedSourceDownload, failure_kind, make_request,
    record_count, scoped,
};
use crate::{
    IndexError, IndexFileOrigin, ValidatedIndexFile,
    download::source::{SourceCandidate, SourceFile},
    parse::{ParsedFile, parse_file},
};

#[cfg(test)]
pub(super) async fn download_file(
    store: &StoreRoot,
    source: SourceFile,
    mode: CheckMode,
) -> Result<PreparedSourceDownload, IndexError> {
    let year = source.year;
    download_file_for_years(store, source, mode, &[year]).await
}

pub(super) async fn download_file_for_years(
    store: &StoreRoot,
    source: SourceFile,
    mode: CheckMode,
    target_years: &[u16],
) -> Result<PreparedSourceDownload, IndexError> {
    crate::download::license::notice(source.dataset);
    let scoped = scoped(store, source.dataset).await?;
    download_file_unlocked_for_years(&scoped, source, mode, target_years).await
}

pub(super) async fn download_file_unlocked_for_years(
    scoped: &ScopedStore,
    source: SourceFile,
    mode: CheckMode,
    target_years: &[u16],
) -> Result<PreparedSourceDownload, IndexError> {
    let mut files = Vec::new();
    let mut failures = Vec::new();
    let mut remote_error = None;
    for candidate in &source.candidates {
        match download_candidate(scoped, &source, candidate, mode, target_years).await {
            Ok(candidate_files) => {
                let complete = candidate_files.iter().any(complete_month);
                files.extend(candidate_files);
                if complete {
                    break;
                }
            }
            Err(error) => {
                failures.push(IntervalFailure::candidate(
                    &source,
                    candidate,
                    failure_kind(&error),
                    error.to_string(),
                ));
                remote_error = Some(error);
            }
        }
    }
    if !files.is_empty() {
        files.sort_by_key(|file| std::cmp::Reverse(file.file.edition.priority()));
        return Ok(PreparedSourceDownload { files, failures });
    }
    if source.cache.is_some() {
        tracing::warn!(
            operation = "indices.cache_fallback",
            dataset = ?source.dataset,
            year = source.year,
            month = source.month,
            remote_error = remote_error.as_ref().map(ToString::to_string),
            "upstream source unavailable; using bundled cache"
        );
        return Ok(PreparedSourceDownload {
            files: download_cached_for_years(scoped, &source, target_years).await?,
            failures,
        });
    }
    if let Some(error) = remote_error {
        return Err(error);
    }
    Err(IndexError::SourceUnavailable {
        dataset: source.dataset,
        year: source.year,
        month: source.month.unwrap_or(0),
    })
}

pub(super) fn complete_month(file: &PreparedIndexFile) -> bool {
    let validated = &file.file;
    let Some(month) = validated.month else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if validated.year.is_multiple_of(4)
            && (!validated.year.is_multiple_of(100) || validated.year.is_multiple_of(400)) =>
        {
            29
        }
        2 => 28,
        _ => return false,
    };
    let expected = days * 24;
    match &file.parsed {
        ParsedFile::Dst(records) | ParsedFile::Ae(records) => {
            records.len() == expected && records.iter().all(|record| record.value.is_some())
        }
        _ => false,
    }
}

async fn download_candidate(
    scoped: &ScopedStore,
    source: &SourceFile,
    candidate: &SourceCandidate,
    mode: CheckMode,
    target_years: &[u16],
) -> Result<Vec<PreparedIndexFile>, IndexError> {
    let request = make_request(source, candidate)?;
    let manager = scoped.downloads()?;
    let download = manager.check(&request, mode).await?;
    let parsed = parse_file(
        source.dataset,
        &download.artifact.path,
        source.year,
        source.month,
    )
    .await?;
    let records = record_count(&parsed);
    let pending = ValidatedIndexFile {
        source_id: candidate.source_id.clone(),
        canonical_url: Some(candidate.url.clone()),
        dataset: source.dataset,
        year: source.year,
        month: source.month,
        edition: candidate.edition,
        records,
        origin: IndexFileOrigin::Upstream,
        download: download.clone(),
    };
    let mut prepared = prepare_candidates(scoped, &pending, &parsed, target_years).await?;
    let accepted = scoped.accept(&request, &download.artifact).await?;
    let accepted_download = ionoray_store::DownloadOutcome {
        artifact: accepted.artifact,
        ..download
    };
    for item in &mut prepared {
        item.file.download = accepted_download.clone();
    }
    Ok(prepared)
}

async fn prepare_candidates(
    scoped: &ScopedStore,
    file: &ValidatedIndexFile,
    parsed: &ParsedFile,
    target_years: &[u16],
) -> Result<Vec<PreparedIndexFile>, IndexError> {
    let mut years = if file.month.is_none() {
        target_years.to_vec()
    } else {
        Vec::new()
    };
    years.push(file.year);
    years.sort_unstable();
    years.dedup();
    let mut prepared = Vec::with_capacity(years.len());
    for year in years {
        let mut partition = file.clone();
        partition.year = year;
        let parsed = if year == file.year {
            parsed.clone()
        } else {
            parse_file(file.dataset, &file.download.artifact.path, year, file.month).await?
        };
        partition.records = record_count(&parsed);
        let path =
            scoped
                .layout()
                .index_database(file.dataset.provider(), file.dataset.as_str(), year);
        if path.exists() {
            let database = crate::database::open_existing_year(path, file.dataset, year).await?;
            crate::import::validate_candidate(&database, &partition, &parsed).await?;
        }
        prepared.push(PreparedIndexFile {
            file: partition,
            parsed,
        });
    }
    Ok(prepared)
}

pub(super) async fn download_cached(
    scoped: &ScopedStore,
    source: &SourceFile,
) -> Result<PreparedIndexFile, IndexError> {
    let year = source.year;
    let mut files = download_cached_for_years(scoped, source, &[year]).await?;
    Ok(files.remove(0))
}

pub(super) async fn download_cached_for_years(
    scoped: &ScopedStore,
    source: &SourceFile,
    target_years: &[u16],
) -> Result<Vec<PreparedIndexFile>, IndexError> {
    let cache = source
        .cache
        .as_ref()
        .expect("caller checked cache availability");
    let actual =
        ionoray_core::Sha256Digest::from_bytes(Sha256::digest(cache.bytes).into()).to_hex();
    if actual != cache.sha256 {
        return Err(IndexError::BundledCacheIntegrity {
            dataset: source.dataset,
            expected: cache.sha256,
            actual,
        });
    }
    let candidate = source
        .candidates
        .first()
        .expect("every source definition has an upstream identity");
    let request = make_request(source, candidate)?;
    let manager = scoped.downloads()?;
    let download = manager
        .ingest_bundled(&request, cache.bytes, cache.source_modified_at_utc_ms)
        .await?;
    let parsed = parse_file(
        source.dataset,
        &download.artifact.path,
        source.year,
        source.month,
    )
    .await?;
    let records = record_count(&parsed);
    let pending = ValidatedIndexFile {
        source_id: candidate.source_id.clone(),
        canonical_url: Some(candidate.url.clone()),
        dataset: source.dataset,
        year: source.year,
        month: source.month,
        edition: candidate.edition,
        records,
        origin: IndexFileOrigin::BundledCache,
        download: download.clone(),
    };
    let mut prepared = prepare_candidates(scoped, &pending, &parsed, target_years).await?;
    let accepted = scoped.accept(&request, &download.artifact).await?;
    let accepted_download = ionoray_store::DownloadOutcome {
        artifact: accepted.artifact,
        ..download
    };
    for item in &mut prepared {
        item.file.download = accepted_download.clone();
    }
    Ok(prepared)
}
