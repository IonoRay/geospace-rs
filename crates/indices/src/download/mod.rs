pub(crate) mod cache;
mod cache_manifest;
mod model;
mod service;
mod source;

pub use model::{
    IndexDataset, IndexEdition, IndexFileOrigin, QueriedIndexFile, ValidatedIndexFile,
    YearDownloadReport, YearQueryReport,
};

pub(crate) use service::{
    IntervalFailure, IntervalFailureKind, PreparedYearDownloadReport,
    download_cached_dataset_year_prepared, download_dataset_interval_prepared, download_year,
    query_year,
};

#[cfg(test)]
mod tests;
