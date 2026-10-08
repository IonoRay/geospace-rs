use ionoray_store::{
    CheckMode, DownloadError, DownloadRequest, RemoteState, ScopedStore, SourceContext, StoreRoot,
    StoreScope,
};
use url::Url;

#[path = "service_transfer.rs"]
mod transfer;
#[cfg(test)]
use transfer::{complete_month, download_file};
use transfer::{download_cached, download_file_for_years, download_file_unlocked_for_years};

use crate::{
    IndexError, QueriedIndexFile, ValidatedIndexFile, YearDownloadReport, YearQueryReport,
    download::source::{
        SourceCandidate, SourceFile, files_for_dataset_interval, files_for_dataset_year,
        files_for_year,
    },
    parse::ParsedFile,
};

/// A validated transport result paired with the single parser output consumed
/// by import. Kept internal so public download reports remain metadata-only.
pub(crate) struct PreparedIndexFile {
    pub(crate) file: ValidatedIndexFile,
    pub(crate) parsed: ParsedFile,
}

pub(crate) struct PreparedYearDownloadReport {
    pub(crate) year: u16,
    pub(crate) files: Vec<PreparedIndexFile>,
}

/// One source partition that failed while other interval partitions continued.
pub(crate) struct IntervalFailure {
    pub(crate) year: u16,
    pub(crate) month: Option<u8>,
    pub(crate) kind: IntervalFailureKind,
    pub(crate) message: String,
    pub(crate) source_id: Option<String>,
    pub(crate) canonical_url: Option<String>,
    pub(crate) edition: Option<crate::IndexEdition>,
}

impl IntervalFailure {
    pub(crate) fn new(
        year: u16,
        month: Option<u8>,
        kind: IntervalFailureKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            year,
            month,
            kind,
            message: message.into(),
            source_id: None,
            canonical_url: None,
            edition: None,
        }
    }

    fn candidate(
        source: &SourceFile,
        candidate: &SourceCandidate,
        kind: IntervalFailureKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            year: source.year,
            month: source.month,
            kind,
            message: message.into(),
            source_id: Some(candidate.source_id.clone()),
            canonical_url: Some(candidate.url.clone()),
            edition: Some(candidate.edition),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntervalFailureKind {
    RemoteUnavailable,
    Network,
    Parse,
    Unsupported,
}

pub(crate) struct PreparedIntervalDownloadReport {
    pub(crate) files: Vec<PreparedIndexFile>,
    pub(crate) failures: Vec<IntervalFailure>,
}

struct PreparedSourceDownload {
    files: Vec<PreparedIndexFile>,
    failures: Vec<IntervalFailure>,
}

impl PreparedYearDownloadReport {
    pub(crate) fn report(&self) -> YearDownloadReport {
        YearDownloadReport {
            year: self.year,
            files: self.files.iter().map(|item| item.file.clone()).collect(),
        }
    }
}

#[tracing::instrument(
    name = "indices.query_year",
    level = "info",
    skip(store),
    fields(year, source_count = tracing::field::Empty),
    err(level = "warn")
)]
pub(crate) async fn query_year(
    store: &StoreRoot,
    year: u16,
) -> Result<YearQueryReport, IndexError> {
    let sources = files_for_year(year)?;
    tracing::Span::current().record("source_count", sources.len());
    let mut files = Vec::new();
    for source in sources {
        let scoped = scoped(store, source.dataset).await?;
        let _guard = scoped.try_acquire_sync_lock()?;
        let manager = scoped.downloads()?;
        let mut selected = false;
        for candidate in &source.candidates {
            let request = make_request(&source, candidate)?;
            let query = manager.query(&request).await?;
            if query.state == RemoteState::Missing {
                continue;
            }
            files.push(QueriedIndexFile {
                dataset: source.dataset,
                year,
                month: source.month,
                edition: candidate.edition,
                source_id: candidate.source_id.clone(),
                query,
            });
            selected = true;
            break;
        }
        if !selected {
            return Err(IndexError::SourceUnavailable {
                dataset: source.dataset,
                year,
                month: source.month.unwrap_or(0),
            });
        }
    }
    tracing::info!(
        operation = "indices.query_year",
        year,
        selected_files = files.len(),
        "year source query completed"
    );
    Ok(YearQueryReport { year, files })
}

#[tracing::instrument(
    name = "indices.download_year",
    level = "info",
    skip(store),
    fields(year, mode = ?mode, source_count = tracing::field::Empty),
    err(level = "warn")
)]
pub(crate) async fn download_year(
    store: &StoreRoot,
    year: u16,
    mode: CheckMode,
) -> Result<YearDownloadReport, IndexError> {
    let sources = files_for_year(year)?;
    tracing::Span::current().record("source_count", sources.len());
    let mut files = Vec::with_capacity(sources.len());
    for source in sources {
        let scoped = scoped(store, source.dataset).await?;
        let _guard = scoped.try_acquire_sync_lock()?;
        let mut years = vec![year];
        years.extend(imported_years(store, source.dataset));
        years.sort_unstable();
        years.dedup();
        files.extend(
            download_file_unlocked_for_years(&scoped, source, mode, &years)
                .await?
                .files,
        );
    }
    tracing::info!(
        operation = "indices.download_year",
        year,
        downloaded_files = files.len(),
        "year download and validation completed"
    );
    Ok(YearDownloadReport {
        year,
        files: files.into_iter().map(|file| file.file).collect(),
    })
}

/// Downloads only physical source files intersecting a requested UTC range.
pub(crate) async fn download_dataset_interval_prepared(
    store: &StoreRoot,
    dataset: crate::IndexDataset,
    start_utc_ms: i64,
    end_utc_ms: i64,
    mode: CheckMode,
) -> Result<PreparedIntervalDownloadReport, IndexError> {
    let sources = files_for_dataset_interval(dataset, start_utc_ms, end_utc_ms)?;
    let mut years = interval_years(start_utc_ms, end_utc_ms)?;
    years.extend(imported_years(store, dataset));
    years.sort_unstable();
    years.dedup();
    download_interval_sources(store, sources, mode, &years).await
}

async fn download_interval_sources(
    store: &StoreRoot,
    sources: Vec<SourceFile>,
    mode: CheckMode,
    target_years: &[u16],
) -> Result<PreparedIntervalDownloadReport, IndexError> {
    let mut files = Vec::with_capacity(sources.len());
    let mut failures = Vec::new();
    for source in sources {
        let year = source.year;
        let month = source.month;
        match download_file_for_years(store, source, mode, target_years).await {
            Ok(prepared) => {
                failures.extend(prepared.failures);
                files.extend(prepared.files);
            }
            Err(error) => failures.push(IntervalFailure::new(
                year,
                month,
                failure_kind(&error),
                error.to_string(),
            )),
        }
    }
    Ok(PreparedIntervalDownloadReport { files, failures })
}

fn failure_kind(error: &IndexError) -> IntervalFailureKind {
    match error {
        IndexError::SourceUnavailable { .. }
        | IndexError::Download(DownloadError::UnexpectedStatus(404 | 410)) => {
            IntervalFailureKind::RemoteUnavailable
        }
        IndexError::Validation { .. } | IndexError::ReadArtifact { .. } => {
            IntervalFailureKind::Parse
        }
        IndexError::UnsupportedYear { .. } => IntervalFailureKind::Unsupported,
        _ => IntervalFailureKind::Network,
    }
}

fn record_count(parsed: &ParsedFile) -> usize {
    match parsed {
        ParsedFile::Gfz { geomagnetic, daily } => geomagnetic.len() + daily.len(),
        ParsedFile::Dst(records) | ParsedFile::Ae(records) => records.len(),
        ParsedFile::IriIgRz(records) => records.len(),
        ParsedFile::IriApF107(records) => records.len(),
    }
}

fn interval_years(start_utc_ms: i64, end_utc_ms: i64) -> Result<Vec<u16>, IndexError> {
    let start = crate::time::epoch_from_millis(start_utc_ms)
        .to_gregorian_utc()
        .0;
    let end = crate::time::epoch_from_millis(end_utc_ms.saturating_sub(1))
        .to_gregorian_utc()
        .0;
    (start..=end)
        .map(|year| u16::try_from(year).map_err(|_| IndexError::InvalidNumber("range year")))
        .collect()
}

fn imported_years(store: &StoreRoot, dataset: crate::IndexDataset) -> Vec<u16> {
    let directory = store
        .layout()
        .root()
        .join("indices")
        .join(dataset.provider())
        .join(dataset.as_str());
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let stem = name.to_str()?.strip_suffix(".db")?;
            stem.parse::<u16>().ok()
        })
        .collect()
}

pub(crate) async fn download_cached_dataset_year_prepared(
    store: &StoreRoot,
    dataset: crate::IndexDataset,
    year: u16,
) -> Result<PreparedYearDownloadReport, IndexError> {
    let sources = files_for_dataset_year(dataset, year)?;
    let mut files = Vec::new();
    for source in sources {
        if source.cache.is_some() {
            let scoped = scoped(store, source.dataset).await?;
            files.push(download_cached(&scoped, &source).await?);
        }
    }
    Ok(PreparedYearDownloadReport { year, files })
}

async fn scoped(
    store: &StoreRoot,
    dataset: crate::IndexDataset,
) -> Result<ScopedStore, IndexError> {
    Ok(store.scoped(StoreScope::new(dataset.scope_name())?).await?)
}

fn make_request(
    source: &SourceFile,
    candidate: &SourceCandidate,
) -> Result<DownloadRequest, IndexError> {
    Ok(DownloadRequest::new(
        candidate.source_id.clone(),
        Url::parse(&candidate.url)?,
        source.logical_name.clone(),
    )?
    .with_context(SourceContext {
        provider: source.dataset.provider().to_owned(),
        dataset: source.dataset.as_str().to_owned(),
        year: source.year,
        month: source.month,
        edition: candidate.edition.as_str().to_owned(),
        priority: candidate.edition.priority(),
    }))
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
