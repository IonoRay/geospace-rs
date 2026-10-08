use reqwest::{Client, Response, StatusCode};
use turso::Database;
use uuid::Uuid;

use crate::{
    StoreLayout,
    cas::{Cas, StoredObject},
    download::{
        ArtifactRef, CheckMode, DownloadDisposition, DownloadError, DownloadObservation,
        DownloadOutcome, DownloadRequest,
        http::{remote_metadata_matches, response_metadata, send_with_validators},
        repository::{
            DownloadRepository, PreviousArtifact, ResponseMetadata, WithoutBodyCompletion,
        },
    },
    time::unix_time_millis,
};

pub(crate) type VerifiedPrevious = (PreviousArtifact, StoredObject);
/// Layered remote checker backed by immutable content-addressed storage.
pub struct DownloadManager<'a> {
    pub(crate) repository: DownloadRepository<'a>,
    pub(crate) cas: Cas<'a>,
    pub(crate) client: Client,
}

impl<'a> DownloadManager<'a> {
    pub(crate) fn new(
        catalog: &'a Database,
        layout: &'a StoreLayout,
    ) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .user_agent(concat!("ionoray-store/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(300))
            .build()?;
        Ok(Self {
            repository: DownloadRepository::new(catalog),
            cas: Cas::new(layout),
            client,
        })
    }

    /// Checks a source with [`CheckMode::Metadata`] and commits changed bytes.
    /// # Errors
    /// Returns [`DownloadError`] for remote, local, or persistence failures.
    pub async fn download(
        &self,
        request: &DownloadRequest,
    ) -> Result<DownloadOutcome, DownloadError> {
        self.check(request, CheckMode::Metadata).await
    }

    /// Checks a source using the selected consistency depth.
    /// [`CheckMode::ForceContent`] hashes the complete local object and performs
    /// an unconditional GET so the remote body is compared by SHA-256.
    /// # Errors
    /// Returns [`DownloadError`] for HTTP, filesystem, integrity, or persistence
    /// failures.
    #[tracing::instrument(
        name = "download.check",
        level = "debug",
        skip_all,
        fields(source_id = request.source_id(), url = %request.url(), mode = ?mode),
        err(level = "warn")
    )]
    pub async fn check(
        &self,
        request: &DownloadRequest,
        mode: CheckMode,
    ) -> Result<DownloadOutcome, DownloadError> {
        let queried_at_utc_ms = unix_time_millis();
        let observation_id = Uuid::now_v7().as_hyphenated().to_string();
        let previous = self
            .resolve_previous(request, mode, &observation_id, queried_at_utc_ms)
            .await?;
        let sent_validators = (mode == CheckMode::Metadata)
            .then_some(previous.as_ref())
            .flatten()
            .map(|item| &item.0);
        self.repository
            .begin(
                &observation_id,
                request,
                mode.as_str(),
                queried_at_utc_ms,
                sent_validators,
            )
            .await?;

        if mode == CheckMode::Metadata
            && let Some(previous_ref) = previous.as_ref()
        {
            let head = self.send_head(request, Some(&previous_ref.0)).await;
            match head {
                Ok(response) if response.status() == StatusCode::NOT_MODIFIED => {
                    let metadata = response_metadata(&response);
                    return self
                        .finish_without_body(
                            request,
                            observation_id,
                            queried_at_utc_ms,
                            previous,
                            &metadata,
                            DownloadDisposition::NotModified,
                        )
                        .await;
                }
                Ok(response) if response.status() == StatusCode::OK => {
                    let metadata = response_metadata(&response);
                    if remote_metadata_matches(&previous_ref.0, &metadata) {
                        return self
                            .finish_without_body(
                                request,
                                observation_id,
                                queried_at_utc_ms,
                                previous,
                                &metadata,
                                DownloadDisposition::MetadataUnchanged,
                            )
                            .await;
                    }
                    return self
                        .send_and_finish_get(
                            request,
                            observation_id,
                            queried_at_utc_ms,
                            previous,
                            false,
                        )
                        .await;
                }
                Ok(response)
                    if matches!(
                        response.status(),
                        StatusCode::FORBIDDEN
                            | StatusCode::METHOD_NOT_ALLOWED
                            | StatusCode::NOT_IMPLEMENTED
                    ) =>
                {
                    return self
                        .send_and_finish_get(
                            request,
                            observation_id,
                            queried_at_utc_ms,
                            previous,
                            true,
                        )
                        .await;
                }
                Ok(response) => {
                    return self
                        .finish_http_error(&observation_id, response.status())
                        .await;
                }
                Err(error) => {
                    self.record_failure(&observation_id, "http", &error, None)
                        .await;
                    return Err(error);
                }
            }
        }

        self.send_and_finish_get(request, observation_id, queried_at_utc_ms, previous, false)
            .await
    }

    /// Returns every persisted request for one stable source identifier.
    /// # Errors
    /// Returns [`DownloadError::Turso`] when history cannot be queried.
    pub async fn history(
        &self,
        source_id: &str,
    ) -> Result<Vec<DownloadObservation>, DownloadError> {
        self.repository.history(source_id).await
    }

    async fn send_and_finish_get(
        &self,
        request: &DownloadRequest,
        observation_id: String,
        queried_at_utc_ms: i64,
        previous: Option<VerifiedPrevious>,
        conditional: bool,
    ) -> Result<DownloadOutcome, DownloadError> {
        let validators = conditional
            .then_some(previous.as_ref())
            .flatten()
            .map(|item| &item.0);
        let response = match self.send_get(request, validators).await {
            Ok(response) => response,
            Err(error) => {
                self.record_failure(&observation_id, "http", &error, None)
                    .await;
                return Err(error);
            }
        };
        let metadata = response_metadata(&response);
        match response.status() {
            StatusCode::NOT_MODIFIED => {
                self.finish_without_body(
                    request,
                    observation_id,
                    queried_at_utc_ms,
                    previous,
                    &metadata,
                    DownloadDisposition::NotModified,
                )
                .await
            }
            StatusCode::OK => {
                self.finish_download(
                    request,
                    observation_id,
                    queried_at_utc_ms,
                    response,
                    metadata,
                )
                .await
            }
            status => self.finish_http_error(&observation_id, status).await,
        }
    }

    async fn finish_without_body(
        &self,
        request: &DownloadRequest,
        observation_id: String,
        queried_at_utc_ms: i64,
        previous: Option<VerifiedPrevious>,
        metadata: &ResponseMetadata,
        disposition: DownloadDisposition,
    ) -> Result<DownloadOutcome, DownloadError> {
        let Some((previous, object)) = previous else {
            let error = DownloadError::NotModifiedWithoutBaseline;
            self.record_failure(
                &observation_id,
                "protocol",
                &error,
                Some(metadata.http_status),
            )
            .await;
            return Err(error);
        };
        let finished_at_utc_ms = unix_time_millis();
        let status = match disposition {
            DownloadDisposition::NotModified => crate::ObservationStatus::NotModified,
            DownloadDisposition::MetadataUnchanged => crate::ObservationStatus::MetadataUnchanged,
            DownloadDisposition::Downloaded | DownloadDisposition::Reused => {
                unreachable!("body dispositions cannot finish without a body")
            }
        };
        let persisted = self
            .repository
            .complete_without_body(&WithoutBodyCompletion {
                observation_id: &observation_id,
                request,
                previous: &previous,
                response: metadata,
                object: &object,
                finished_at_utc_ms,
                status,
            })
            .await;
        if let Err(error) = persisted {
            self.record_failure(
                &observation_id,
                "persistence",
                &error,
                Some(metadata.http_status),
            )
            .await;
            return Err(error);
        }
        tracing::debug!(
            operation = "download.check",
            status = ?disposition,
            artifact_sha256 = %previous.digest,
            bytes = 0_u64,
            elapsed_ms = finished_at_utc_ms.saturating_sub(queried_at_utc_ms),
            "remote content reused without body transfer"
        );
        Ok(DownloadOutcome {
            observation_id,
            disposition,
            artifact: previous.to_artifact_ref(&object),
            queried_at_utc_ms,
            finished_at_utc_ms,
        })
    }

    async fn finish_download(
        &self,
        request: &DownloadRequest,
        observation_id: String,
        queried_at_utc_ms: i64,
        response: Response,
        metadata: ResponseMetadata,
    ) -> Result<DownloadOutcome, DownloadError> {
        if let Err(error) = self
            .repository
            .start_download(&observation_id, unix_time_millis())
            .await
        {
            self.record_failure(
                &observation_id,
                "persistence",
                &error,
                Some(metadata.http_status),
            )
            .await;
            return Err(error);
        }
        let object = match self
            .cas
            .ingest_response(response, metadata.content_length)
            .await
        {
            Ok(object) => object,
            Err(error) => {
                self.record_failure(&observation_id, "body", &error, Some(metadata.http_status))
                    .await;
                return Err(error);
            }
        };
        let finished_at_utc_ms = unix_time_millis();
        let downloaded_at_utc_ms = match self
            .repository
            .complete_download(
                &observation_id,
                request,
                &object,
                &metadata,
                finished_at_utc_ms,
            )
            .await
        {
            Ok(downloaded_at_utc_ms) => downloaded_at_utc_ms,
            Err(error) => {
                self.record_failure(
                    &observation_id,
                    "persistence",
                    &error,
                    Some(metadata.http_status),
                )
                .await;
                return Err(error);
            }
        };
        let disposition = if object.created {
            DownloadDisposition::Downloaded
        } else {
            DownloadDisposition::Reused
        };
        tracing::debug!(
            operation = "download.check",
            status = ?disposition,
            artifact_sha256 = %object.digest,
            bytes = object.byte_size,
            elapsed_ms = finished_at_utc_ms.saturating_sub(queried_at_utc_ms),
            "remote body committed"
        );
        Ok(DownloadOutcome {
            observation_id,
            disposition,
            artifact: ArtifactRef {
                digest: object.digest,
                byte_size: object.byte_size,
                path: object.path,
                original_filename: request.logical_name().to_owned(),
                media_type: metadata.media_type,
                source_modified_at_utc_ms: metadata.last_modified_utc_ms,
                downloaded_at_utc_ms,
                local_mtime_ns: object.local_mtime_ns,
            },
            queried_at_utc_ms,
            finished_at_utc_ms,
        })
    }

    pub(crate) async fn verified_previous(
        &self,
        request: &DownloadRequest,
        force_hash: bool,
    ) -> Result<Option<VerifiedPrevious>, DownloadError> {
        let previous = self.repository.latest_baseline(request.source_id()).await?;
        let Some(previous) = previous else {
            return Ok(None);
        };
        let object = self
            .cas
            .verified_object(
                previous.digest,
                previous.byte_size,
                previous.local_mtime_ns,
                previous.downloaded_at_utc_ms,
                force_hash,
            )
            .await?;
        Ok(object.map(|object| (previous, object)))
    }

    async fn resolve_previous(
        &self,
        request: &DownloadRequest,
        mode: CheckMode,
        observation_id: &str,
        queried_at_utc_ms: i64,
    ) -> Result<Option<VerifiedPrevious>, DownloadError> {
        match self
            .verified_previous(request, mode == CheckMode::ForceContent)
            .await
        {
            Ok(previous) => Ok(previous),
            Err(DownloadError::CorruptObject { path, expected })
                if mode == CheckMode::ForceContent =>
            {
                tracing::warn!(path = %path.display(), digest = %expected, "discarding corrupt CAS object before forced redownload");
                self.cas.discard_corrupt(&path).await?;
                Ok(None)
            }
            Err(error) => {
                self.persist_integrity_failure(observation_id, request, queried_at_utc_ms, &error)
                    .await;
                Err(error)
            }
        }
    }

    pub(crate) async fn send_head(
        &self,
        request: &DownloadRequest,
        previous: Option<&PreviousArtifact>,
    ) -> Result<Response, DownloadError> {
        send_with_validators(self.client.head(request.url().clone()), previous).await
    }

    async fn send_get(
        &self,
        request: &DownloadRequest,
        previous: Option<&PreviousArtifact>,
    ) -> Result<Response, DownloadError> {
        send_with_validators(self.client.get(request.url().clone()), previous).await
    }

    async fn finish_http_error(
        &self,
        observation_id: &str,
        status: StatusCode,
    ) -> Result<DownloadOutcome, DownloadError> {
        let error = DownloadError::UnexpectedStatus(status.as_u16());
        self.record_failure(observation_id, "http_status", &error, Some(status.as_u16()))
            .await;
        Err(error)
    }

    async fn persist_integrity_failure(
        &self,
        observation_id: &str,
        request: &DownloadRequest,
        queried_at_utc_ms: i64,
        error: &DownloadError,
    ) {
        if self
            .repository
            .begin(
                observation_id,
                request,
                CheckMode::Metadata.as_str(),
                queried_at_utc_ms,
                None,
            )
            .await
            .is_ok()
        {
            self.record_failure(observation_id, "integrity", error, None)
                .await;
        }
    }

    pub(crate) async fn record_failure(
        &self,
        observation_id: &str,
        kind: &str,
        error: &DownloadError,
        http_status: Option<u16>,
    ) {
        let _ = self
            .repository
            .fail(
                observation_id,
                kind,
                &error.to_string(),
                unix_time_millis(),
                http_status,
            )
            .await;
    }
}
