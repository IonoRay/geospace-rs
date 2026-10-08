use std::time::Duration;

use httpdate::parse_http_date;
use reqwest::{
    RequestBuilder, Response, StatusCode,
    header::{
        CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH,
        LAST_MODIFIED, RETRY_AFTER,
    },
};

const REQUEST_ATTEMPTS: u32 = 3;

pub(crate) async fn send_with_validators(
    mut builder: RequestBuilder,
    previous: Option<&PreviousArtifact>,
) -> Result<Response, crate::DownloadError> {
    if let Some(etag) = previous.and_then(|item| item.etag.as_deref()) {
        builder = builder.header(IF_NONE_MATCH, etag);
    }
    if let Some(modified) = previous.and_then(|item| item.last_modified_raw.as_deref()) {
        builder = builder.header(IF_MODIFIED_SINCE, modified);
    }
    let template = builder
        .try_clone()
        .expect("GET and HEAD requests do not contain non-cloneable bodies");
    for attempt in 0..REQUEST_ATTEMPTS {
        let request = template
            .try_clone()
            .expect("request template remains cloneable");
        match request.send().await {
            Ok(response)
                if is_retryable_status(response.status()) && attempt + 1 < REQUEST_ATTEMPTS =>
            {
                let Some(delay) = retry_delay(response.headers().get(RETRY_AFTER), attempt) else {
                    return Ok(response);
                };
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if is_transient(&error) && attempt + 1 < REQUEST_ATTEMPTS => {
                tokio::time::sleep(
                    retry_delay(None, attempt).expect("no Retry-After always retries"),
                )
                .await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("request loop always returns on its final attempt")
}

use crate::{
    download::repository::{PreviousArtifact, ResponseMetadata},
    time::system_time_millis,
};

pub(crate) fn remote_metadata_matches(
    previous: &PreviousArtifact,
    current: &ResponseMetadata,
) -> bool {
    let same_url = previous.final_url.as_deref() == Some(current.final_url.as_str());
    let same_size = current.content_length == Some(previous.byte_size);
    let comparable_etag = previous.etag.is_some() && current.etag.is_some();
    let same_etag = comparable_etag && previous.etag == current.etag;
    let same_modified = previous.last_modified_raw.is_some()
        && previous.last_modified_raw == current.last_modified_raw;
    same_url
        && same_size
        && if comparable_etag {
            same_etag
        } else {
            same_modified
        }
}

pub(crate) fn response_metadata(response: &Response) -> ResponseMetadata {
    let last_modified_raw = header_text(response, LAST_MODIFIED);
    let last_modified_utc_ms = last_modified_raw
        .as_deref()
        .and_then(|value| parse_http_date(value).ok())
        .map(system_time_millis);
    ResponseMetadata {
        final_url: response.url().to_string(),
        http_status: response.status().as_u16(),
        etag: header_text(response, ETAG),
        last_modified_raw,
        last_modified_utc_ms,
        content_length: header_text(response, CONTENT_LENGTH)
            .and_then(|value| value.parse().ok())
            .or_else(|| response.content_length()),
        media_type: header_text(response, CONTENT_TYPE),
        content_disposition: header_text(response, CONTENT_DISPOSITION),
    }
}

fn header_text(response: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn is_transient(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retry_delay(
    retry_after: Option<&reqwest::header::HeaderValue>,
    attempt: u32,
) -> Option<Duration> {
    let fallback = Duration::from_millis(200 * 4_u64.pow(attempt));
    let Some(value) = retry_after.and_then(|value| value.to_str().ok()) else {
        return Some(fallback);
    };
    if let Ok(seconds) = value.parse::<u64>() {
        return (seconds <= 30).then(|| Duration::from_secs(seconds));
    }
    parse_http_date(value)
        .ok()
        .and_then(|when| when.duration_since(std::time::SystemTime::now()).ok())
        .map_or(Some(fallback), |delay| {
            (delay <= Duration::from_secs(30)).then_some(delay)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::repository::{PreviousArtifact, ResponseMetadata};

    #[test]
    fn changed_etag_is_not_masked_by_an_equal_last_modified() {
        let previous = PreviousArtifact {
            digest: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
            byte_size: 4,
            original_filename: "test".to_owned(),
            media_type: None,
            source_modified_at_utc_ms: None,
            etag: Some("old".to_owned()),
            last_modified_raw: Some("Tue, 15 Nov 1994 08:12:31 GMT".to_owned()),
            final_url: Some("https://example.test/file".to_owned()),
            downloaded_at_utc_ms: 0,
            local_mtime_ns: 0,
        };
        let current = ResponseMetadata {
            final_url: "https://example.test/file".to_owned(),
            http_status: 200,
            etag: Some("new".to_owned()),
            last_modified_raw: previous.last_modified_raw.clone(),
            last_modified_utc_ms: None,
            content_length: Some(4),
            media_type: None,
            content_disposition: None,
        };
        assert!(!remote_metadata_matches(&previous, &current));
    }
}
