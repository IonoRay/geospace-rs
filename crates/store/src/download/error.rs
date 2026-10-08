use std::{io, path::PathBuf};

use ionoray_core::{DigestError, Sha256Digest};
use thiserror::Error;

/// Failures while checking, downloading, or committing a source artifact.
#[derive(Debug, Error)]
pub enum DownloadError {
    /// Source identifiers are stable database keys with a restricted alphabet.
    #[error("invalid source id {0:?}; use 1-128 ASCII letters, digits, '.', '_', ':', or '-'")]
    InvalidSourceId(String),
    /// Logical names are portable filenames, not paths.
    #[error("invalid logical filename {0:?}")]
    InvalidLogicalName(String),
    /// Only HTTP sources are supported by this downloader.
    #[error("unsupported download URL scheme {0:?}; expected http or https")]
    UnsupportedScheme(String),
    /// The remote server returned a non-success response.
    #[error("source returned unexpected HTTP status {0}")]
    UnexpectedStatus(u16),
    /// A server returned 304 when no verified local baseline existed.
    #[error("source returned HTTP 304 without a verified local artifact")]
    NotModifiedWithoutBaseline,
    /// The received body length did not match the declared length.
    #[error("downloaded {actual} bytes but Content-Length declared {expected}")]
    ContentLengthMismatch {
        /// Header value.
        expected: u64,
        /// Streamed byte count.
        actual: u64,
    },
    /// Existing bytes under a content identity no longer match that identity.
    #[error("CAS object {path} does not match {expected}")]
    CorruptObject {
        /// Corrupt local path.
        path: PathBuf,
        /// Digest encoded by the path.
        expected: Sha256Digest,
    },
    /// A CAS path existed but was not a regular file.
    #[error("CAS object path is not a regular file: {0}")]
    InvalidObjectType(PathBuf),
    /// A persisted observation contained an unknown status.
    #[error("unknown download observation status {0:?}")]
    UnknownObservationStatus(String),
    /// A persisted successful observation was incomplete.
    #[error("successful download observation is missing {0}")]
    MissingObservationField(&'static str),
    /// A filesystem or HTTP size cannot be represented by Turso INTEGER.
    #[error("{0} exceeds the supported 64-bit database range")]
    NumericOverflow(&'static str),
    /// Filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Related path.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: io::Error,
    },
    /// HTTP client or response streaming failed.
    #[error("HTTP download failed: {0}")]
    Http(#[from] reqwest::Error),
    /// Turso metadata persistence failed.
    #[error("Turso download metadata operation failed: {0}")]
    Turso(#[from] turso::Error),
    /// Store durability operation failed after metadata commit.
    #[error(transparent)]
    Store(#[from] crate::StoreError),
    /// A stored digest could not be decoded.
    #[error(transparent)]
    Digest(#[from] DigestError),
}

impl DownloadError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
