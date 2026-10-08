use std::{io, path::PathBuf};

use thiserror::Error;

/// Failures while discovering, initializing, or opening the `IonoRay` store.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The home directory could not be discovered.
    #[error("cannot determine the user home directory; set IONORAY_HOME or pass an explicit path")]
    HomeUnavailable,
    /// A configured store path must be absolute to avoid process-dependent state.
    #[error("IonoRay home must be absolute: {0}")]
    RelativeHome(PathBuf),
    /// Existing legacy data makes the implicit default ambiguous.
    #[error(
        "existing legacy IonoRay data at {legacy_home} requires an explicit selection; pass --home (or the API path) or set IONORAY_HOME to {legacy_home} or {default_home}"
    )]
    LegacyHomeNeedsSelection {
        /// Existing legacy root containing `indices` or `objects`.
        legacy_home: PathBuf,
        /// Current implicit default root.
        default_home: PathBuf,
    },
    /// A database path cannot be represented for the Turso API.
    #[error("database path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    /// Filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Related filesystem path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Turso database operation failed.
    #[error("Turso operation failed: {0}")]
    Turso(#[from] turso::Error),
    /// A schema inspection query returned an impossible shape.
    #[error("database schema metadata is missing")]
    MissingSchemaMetadata,
    /// A WAL checkpoint did not return its required status row.
    #[error("Turso WAL checkpoint did not return status metadata")]
    MissingCheckpointStatus,
    /// A WAL checkpoint could not flush every committed frame.
    #[error(
        "Turso WAL checkpoint remained busy (busy={busy}, log={log_frames}, checkpointed={checkpointed_frames})"
    )]
    CheckpointBusy {
        /// SQLite-compatible busy indicator.
        busy: i64,
        /// Frames remaining in the WAL.
        log_frames: i64,
        /// Frames copied into the database file.
        checkpointed_frames: i64,
    },
    /// A maintenance counter exceeded the Turso INTEGER range.
    #[error("maintenance counter exceeds the supported database range")]
    NumericOverflow,
    /// A dataset scope is not a portable directory name.
    #[error("invalid store scope {0:?}; use lowercase letters, digits, and hyphens")]
    InvalidScope(String),
    /// A canonical URL cannot be represented as a managed browse path.
    #[error("invalid canonical URL for browse view: {0}")]
    InvalidCanonicalUrl(String),
    /// A catalog browse path would escape its scoped object directory.
    #[error("invalid catalog browse path: {0}")]
    InvalidBrowsePath(PathBuf),
    /// The caller attempted to accept bytes outside this scoped CAS.
    #[error("accepted artifact is not a verified scoped CAS object: {0}")]
    InvalidAcceptedArtifact(PathBuf),
    /// A browse repair must never replace a user-owned regular file.
    #[error("refusing to overwrite non-symlink browse entry: {0}")]
    RefuseOverwriteView(PathBuf),
    /// Another process holds a read or synchronization guard for this scope.
    #[error("store scope is busy: {0}")]
    ScopeBusy(String),
    /// The system clock predates the Unix epoch.
    #[error("system clock is before the Unix epoch")]
    Clock(std::time::Duration),
}

impl StoreError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
