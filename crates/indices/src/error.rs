use std::path::PathBuf;

use thiserror::Error;

use crate::{IndexDataset, IndexValueError};
use ionoray_core::DigestError;

/// Data access failures from the independent index product.
#[derive(Debug, Error)]
pub enum IndexError {
    /// Local data infrastructure failed.
    #[error(transparent)]
    Store(#[from] ionoray_store::StoreError),
    /// Source download or CAS validation failed.
    #[error(transparent)]
    Download(#[from] ionoray_store::DownloadError),
    /// A source URL definition was invalid.
    #[error(transparent)]
    Url(#[from] url::ParseError),
    /// A yearly Turso database operation failed.
    #[error(transparent)]
    Turso(#[from] turso::Error),
    /// Local yearly database filesystem operation failed.
    #[error("I/O error at {path}: {source}")]
    Io {
        /// Affected path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A numeric database field could not be represented safely.
    #[error("invalid numeric database field {0}")]
    InvalidNumber(&'static str),
    /// A yearly WAL checkpoint did not return status metadata.
    #[error("yearly Turso WAL checkpoint did not return status metadata")]
    MissingCheckpointStatus,
    /// A yearly WAL checkpoint could not flush every committed frame.
    #[error(
        "yearly Turso WAL checkpoint remained busy (busy={busy}, log={log_frames}, checkpointed={checkpointed_frames})"
    )]
    CheckpointBusy {
        /// SQLite-compatible busy indicator.
        busy: i64,
        /// Frames remaining in the WAL.
        log_frames: i64,
        /// Frames copied into the database file.
        checkpointed_frames: i64,
    },
    /// Parsed or queried index value violated its domain contract.
    #[error(transparent)]
    Value(#[from] IndexValueError),
    /// Persisted content digest was invalid.
    #[error(transparent)]
    Digest(#[from] DigestError),
    /// A requested year has not been synchronized locally.
    #[error("local {dataset:?} database for {year} is unavailable")]
    MissingYearData {
        /// Missing dataset.
        dataset: IndexDataset,
        /// Missing calendar year.
        year: u16,
    },
    /// An index timestamp has no ready release covering it.
    #[error("no ready {dataset:?} value exists at {epoch_utc_ms}")]
    MissingValue {
        /// Missing dataset.
        dataset: IndexDataset,
        /// Unix millisecond query time.
        epoch_utc_ms: i64,
    },
    /// A downloaded artifact could not be read for domain validation.
    #[error("cannot read downloaded index artifact {path}: {source}")]
    ReadArtifact {
        /// CAS path.
        path: PathBuf,
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The requested year predates one of the supported datasets.
    #[error("{dataset:?} is unavailable for requested year {year}")]
    UnsupportedYear {
        /// Dataset without coverage.
        dataset: IndexDataset,
        /// Requested year.
        year: u16,
    },
    /// No final, provisional, or realtime source candidate existed.
    #[error("no remote {dataset:?} source is available for {year}-{month:02}")]
    SourceUnavailable {
        /// Missing dataset.
        dataset: IndexDataset,
        /// Requested year.
        year: u16,
        /// Requested month.
        month: u8,
    },
    /// Downloaded bytes did not cover the requested period or format.
    #[error("invalid {dataset:?} artifact {path}: {reason}")]
    Validation {
        /// Dataset being validated.
        dataset: IndexDataset,
        /// CAS path.
        path: PathBuf,
        /// Specific format or coverage failure.
        reason: String,
    },
    /// Packaged fallback bytes no longer match the release manifest.
    #[error("bundled {dataset:?} cache digest is {actual}, expected {expected}")]
    BundledCacheIntegrity {
        /// Dataset whose packaged bytes failed verification.
        dataset: IndexDataset,
        /// Digest pinned by the crate release.
        expected: &'static str,
        /// Digest computed from packaged bytes.
        actual: String,
    },
}
