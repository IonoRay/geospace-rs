use thiserror::Error;

/// Additional activity-product download or parsing failure.
#[derive(Debug, Error)]
pub enum ActivityError {
    /// Invalid request or unrecognized source format.
    #[error("activity data validation failed: {0}")]
    Invalid(String),
    /// Requested interval lacks accepted local files.
    #[error("no accepted local {dataset} file for {partition}")]
    Missing {
        /// Dataset name.
        dataset: &'static str,
        /// UTC date or year.
        partition: String,
    },
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Shared local store failure.
    #[error(transparent)]
    Store(#[from] ionoray_store::StoreError),
    /// HTTP or CAS failure, distinct from absent source files.
    #[error(transparent)]
    Download(#[from] ionoray_store::DownloadError),
    /// Invalid source URL.
    #[error(transparent)]
    Url(#[from] url::ParseError),
}
