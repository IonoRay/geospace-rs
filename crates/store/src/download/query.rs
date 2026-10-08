use reqwest::StatusCode;
use uuid::Uuid;

use crate::{
    download::{
        DownloadError, DownloadManager, DownloadRequest, ObservationStatus, RemoteQueryOutcome,
        RemoteState,
        http::{remote_metadata_matches, response_metadata},
        repository::{ResponseMetadata, WithoutBodyCompletion},
        service::VerifiedPrevious,
    },
    time::unix_time_millis,
};

impl DownloadManager<'_> {
    /// Queries remote metadata without downloading a response body.
    ///
    /// The query is always persisted. Servers that do not support a useful
    /// `HEAD` response yield [`RemoteState::NeedsContentCheck`].
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] for HTTP, local integrity, or persistence failures.
    #[tracing::instrument(
        name = "download.query",
        level = "debug",
        skip_all,
        fields(source_id = request.source_id(), url = %request.url()),
        err(level = "warn")
    )]
    pub async fn query(
        &self,
        request: &DownloadRequest,
    ) -> Result<RemoteQueryOutcome, DownloadError> {
        let queried_at_utc_ms = unix_time_millis();
        let observation_id = Uuid::now_v7().as_hyphenated().to_string();
        let previous = self.verified_previous(request, false).await?;
        self.repository
            .begin(
                &observation_id,
                request,
                "query",
                queried_at_utc_ms,
                previous.as_ref().map(|item| &item.0),
            )
            .await?;
        let response = match self
            .send_head(request, previous.as_ref().map(|item| &item.0))
            .await
        {
            Ok(response) => response,
            Err(error) => {
                self.record_failure(&observation_id, "http", &error, None)
                    .await;
                return Err(error);
            }
        };
        let metadata = response_metadata(&response);
        let state = match response.status() {
            StatusCode::NOT_MODIFIED => RemoteState::Unchanged,
            StatusCode::OK => previous.as_ref().map_or(RemoteState::Available, |item| {
                if remote_metadata_matches(&item.0, &metadata) {
                    RemoteState::Unchanged
                } else {
                    RemoteState::MetadataChanged
                }
            }),
            StatusCode::NOT_FOUND | StatusCode::GONE => RemoteState::Missing,
            StatusCode::FORBIDDEN
            | StatusCode::METHOD_NOT_ALLOWED
            | StatusCode::NOT_IMPLEMENTED => RemoteState::NeedsContentCheck,
            status => {
                let error = DownloadError::UnexpectedStatus(status.as_u16());
                self.record_failure(
                    &observation_id,
                    "http_status",
                    &error,
                    Some(status.as_u16()),
                )
                .await;
                return Err(error);
            }
        };
        let finished_at_utc_ms = unix_time_millis();
        self.persist_query_outcome(
            request,
            &observation_id,
            previous,
            &metadata,
            state,
            finished_at_utc_ms,
        )
        .await?;
        tracing::debug!(
            operation = "download.query",
            status = ?state,
            http_status = metadata.http_status,
            elapsed_ms = finished_at_utc_ms.saturating_sub(queried_at_utc_ms),
            "upstream metadata query completed"
        );
        Ok(RemoteQueryOutcome {
            observation_id,
            state,
            http_status: metadata.http_status,
            etag: metadata.etag,
            last_modified: metadata.last_modified_raw,
            content_length: metadata.content_length,
            queried_at_utc_ms,
            finished_at_utc_ms,
        })
    }

    async fn persist_query_outcome(
        &self,
        request: &DownloadRequest,
        observation_id: &str,
        previous: Option<VerifiedPrevious>,
        metadata: &ResponseMetadata,
        state: RemoteState,
        finished_at_utc_ms: i64,
    ) -> Result<(), DownloadError> {
        if state == RemoteState::Unchanged {
            let Some((previous, object)) = previous else {
                let error = DownloadError::NotModifiedWithoutBaseline;
                self.record_failure(
                    observation_id,
                    "protocol",
                    &error,
                    Some(metadata.http_status),
                )
                .await;
                return Err(error);
            };
            self.repository
                .complete_without_body(&WithoutBodyCompletion {
                    observation_id,
                    request,
                    previous: &previous,
                    response: metadata,
                    object: &object,
                    finished_at_utc_ms,
                    status: ObservationStatus::Available,
                })
                .await?;
        } else {
            self.repository
                .complete_query(
                    observation_id,
                    metadata,
                    finished_at_utc_ms,
                    query_status(state),
                )
                .await?;
        }
        Ok(())
    }
}

const fn query_status(state: RemoteState) -> ObservationStatus {
    match state {
        RemoteState::Unchanged | RemoteState::Available => ObservationStatus::Available,
        RemoteState::MetadataChanged => ObservationStatus::MetadataChanged,
        RemoteState::Missing => ObservationStatus::Missing,
        RemoteState::NeedsContentCheck => ObservationStatus::NeedsContentCheck,
    }
}
