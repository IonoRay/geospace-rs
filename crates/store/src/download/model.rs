use std::path::PathBuf;

use ionoray_core::Sha256Digest;
use serde::{Deserialize, Serialize};
use url::Url;

use super::DownloadError;

/// A stable source identity and one remote file to check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRequest {
    source_id: String,
    url: Url,
    logical_name: String,
    context: Option<SourceContext>,
}

/// Domain-neutral metadata connecting a remote file to a dataset partition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceContext {
    /// Stable upstream provider name.
    pub provider: String,
    /// Stable dataset name within the provider.
    pub dataset: String,
    /// Calendar partition.
    pub year: u16,
    /// Optional monthly partition.
    pub month: Option<u8>,
    /// Upstream publication edition.
    pub edition: String,
    /// Higher values are preferred when resolving candidates.
    pub priority: i32,
}

/// Depth of one remote consistency check.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CheckMode {
    /// Prefer local and remote metadata, with conditional GET as a fallback.
    #[default]
    Metadata,
    /// Fully hash local bytes and unconditionally download remote bytes.
    ForceContent,
}

impl DownloadRequest {
    /// Creates a validated request.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] for a non-portable source identifier, filename,
    /// or non-HTTP URL.
    pub fn new(
        source_id: impl Into<String>,
        url: Url,
        logical_name: impl Into<String>,
    ) -> Result<Self, DownloadError> {
        let source_id = source_id.into();
        let logical_name = logical_name.into();
        if source_id.is_empty()
            || source_id.len() > 128
            || !source_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        {
            return Err(DownloadError::InvalidSourceId(source_id));
        }
        if logical_name.is_empty()
            || logical_name.len() > 255
            || logical_name == "."
            || logical_name == ".."
            || logical_name.contains('/')
            || logical_name.contains('\\')
        {
            return Err(DownloadError::InvalidLogicalName(logical_name));
        }
        if !matches!(url.scheme(), "http" | "https") {
            return Err(DownloadError::UnsupportedScheme(url.scheme().to_owned()));
        }
        Ok(Self {
            source_id,
            url,
            logical_name,
            context: None,
        })
    }

    /// Attaches dataset partition metadata used by the object-origin catalog.
    #[must_use]
    pub fn with_context(mut self, context: SourceContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Stable source key.
    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    /// Remote URL checked on every request.
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Portable original filename recorded with each artifact.
    pub fn logical_name(&self) -> &str {
        &self.logical_name
    }

    /// Dataset partition metadata, when supplied by the data product.
    pub const fn context(&self) -> Option<&SourceContext> {
        self.context.as_ref()
    }
}

impl CheckMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::ForceContent => "force_content",
        }
    }
}

/// How one completed request affected local content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DownloadDisposition {
    /// New bytes created a new CAS object.
    Downloaded,
    /// Conditional HTTP validation returned 304; no body was transferred.
    NotModified,
    /// Remote URL, validators, and size matched without transferring a body.
    MetadataUnchanged,
    /// The server returned a body whose digest was already present.
    Reused,
}

/// Persisted state of one source observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ObservationStatus {
    /// Request was recorded but has not completed.
    Querying,
    /// Remote metadata matched the local baseline.
    Available,
    /// Remote metadata indicates changed content.
    MetadataChanged,
    /// The remote object does not currently exist.
    Missing,
    /// The server cannot establish content state without a body request.
    NeedsContentCheck,
    /// New CAS bytes were committed.
    Downloaded,
    /// Conditional request returned HTTP 304.
    NotModified,
    /// A metadata comparison established that the remote file was unchanged.
    MetadataUnchanged,
    /// A transferred body matched an existing CAS object.
    Unchanged,
    /// Request or persistence failed.
    Failed,
}

impl ObservationStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Querying => "querying",
            Self::Available => "available",
            Self::MetadataChanged => "metadata_changed",
            Self::Missing => "missing",
            Self::NeedsContentCheck => "needs_content_check",
            Self::Downloaded => "downloaded",
            Self::NotModified => "not_modified",
            Self::MetadataUnchanged => "metadata_unchanged",
            Self::Unchanged => "unchanged",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn from_str(value: &str) -> Result<Self, DownloadError> {
        match value {
            "querying" => Ok(Self::Querying),
            "available" => Ok(Self::Available),
            "metadata_changed" => Ok(Self::MetadataChanged),
            "missing" => Ok(Self::Missing),
            "needs_content_check" => Ok(Self::NeedsContentCheck),
            "downloaded" => Ok(Self::Downloaded),
            "not_modified" => Ok(Self::NotModified),
            "metadata_unchanged" => Ok(Self::MetadataUnchanged),
            "unchanged" => Ok(Self::Unchanged),
            "failed" => Ok(Self::Failed),
            _ => Err(DownloadError::UnknownObservationStatus(value.to_owned())),
        }
    }
}

/// Result of a metadata-only upstream query.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemoteState {
    /// A verified local artifact still matches the remote metadata.
    Unchanged,
    /// A remote object exists and no local baseline is available.
    Available,
    /// Remote metadata differs from the local baseline.
    MetadataChanged,
    /// The remote object returned HTTP 404 or 410.
    Missing,
    /// A body request is needed because HEAD is unsupported or inconclusive.
    NeedsContentCheck,
}

/// Auditable metadata-only remote query.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteQueryOutcome {
    /// Persisted observation identifier.
    pub observation_id: String,
    /// Remote comparison state.
    pub state: RemoteState,
    /// HTTP response status.
    pub http_status: u16,
    /// Remote `ETag`, when available.
    pub etag: Option<String>,
    /// Raw remote Last-Modified value.
    pub last_modified: Option<String>,
    /// Declared content size.
    pub content_length: Option<u64>,
    /// Query start time.
    pub queried_at_utc_ms: i64,
    /// Query completion time.
    pub finished_at_utc_ms: i64,
}

/// Immutable local artifact and its relevant remote and filesystem metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Content identity.
    pub digest: Sha256Digest,
    /// Exact byte size.
    pub byte_size: u64,
    /// Absolute local CAS path.
    pub path: PathBuf,
    /// Original portable filename.
    pub original_filename: String,
    /// HTTP content type when supplied.
    pub media_type: Option<String>,
    /// Parsed HTTP Last-Modified timestamp.
    pub source_modified_at_utc_ms: Option<i64>,
    /// Time local bytes finished downloading.
    pub downloaded_at_utc_ms: i64,
    /// Local filesystem modification timestamp in Unix nanoseconds.
    pub local_mtime_ns: i64,
}

/// Result of one complete source check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadOutcome {
    /// Unique observation identifier.
    pub observation_id: String,
    /// Whether bytes were created, skipped by HTTP, or reused by digest.
    pub disposition: DownloadDisposition,
    /// Verified local artifact.
    pub artifact: ArtifactRef,
    /// Time the source query began.
    pub queried_at_utc_ms: i64,
    /// Time the source query completed.
    pub finished_at_utc_ms: i64,
}

/// Auditable record for every source request, including failures and skips.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadObservation {
    /// Unique observation identifier.
    pub observation_id: String,
    /// Stable source key.
    pub source_id: String,
    /// Requested URL at the time of observation.
    pub request_url: String,
    /// Query creation time.
    pub queried_at_utc_ms: i64,
    /// Body download start time, when applicable.
    pub download_started_at_utc_ms: Option<i64>,
    /// Terminal time.
    pub download_finished_at_utc_ms: Option<i64>,
    /// Conditional `If-None-Match` value sent with the request.
    pub request_etag: Option<String>,
    /// Conditional `If-Modified-Since` value sent with the request.
    pub request_last_modified: Option<String>,
    /// Final URL after redirects.
    pub final_url: Option<String>,
    /// HTTP status code.
    pub http_status: Option<u16>,
    /// Response `ETag`.
    pub response_etag: Option<String>,
    /// Raw HTTP Last-Modified value.
    pub last_modified_raw: Option<String>,
    /// Parsed Last-Modified value.
    pub last_modified_utc_ms: Option<i64>,
    /// Declared body size.
    pub content_length_header: Option<u64>,
    /// Actually transferred body bytes.
    pub bytes_received: Option<u64>,
    /// Resulting content identity.
    pub artifact: Option<Sha256Digest>,
    /// State machine status.
    pub status: ObservationStatus,
    /// Stable failure category.
    pub error_kind: Option<String>,
    /// Diagnostic failure text.
    pub error_message: Option<String>,
}
