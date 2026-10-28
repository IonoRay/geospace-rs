use thiserror::Error;

/// A download, CDF validation, or offline coverage failure.
#[derive(Debug, Error)]
pub enum OmniError {
    /// Invalid time interval or unsupported CDF content.
    #[error("OMNI validation failed: {0}")]
    Invalid(String),
    /// Neither product has the requested month.
    #[error("no OMNI one-minute file for {year}-{month:02} (tried {products})")]
    MissingMonth {
        /// UTC year.
        year: u16,
        /// UTC month.
        month: u8,
        /// Explicit product search order.
        products: String,
    },
    /// Local filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Store catalog, home, or integrity failure.
    #[error(transparent)]
    Store(#[from] ionoray_store::StoreError),
    /// HTTP or download failure; never silently falls back to another product.
    #[error(transparent)]
    Download(#[from] ionoray_store::DownloadError),
    /// Direct cdf-rs decoding failed.
    #[error("cdf-rs could not decode OMNI: {0}")]
    Cdf(#[from] cdf::error::CdfError),
}
