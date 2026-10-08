//! End-to-end download state machine tests using a local HTTP server.

use std::{
    fmt::Write as _,
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    CheckMode, DownloadDisposition, DownloadRequest, ObservationStatus, RemoteState, SourceContext,
    Store,
};
use tempfile::TempDir;
use url::Url;

const BODY: &str = "kp,ap,f107\n2,7,120\n";
const LAST_MODIFIED: &str = "Wed, 08 Jul 2026 12:34:56 GMT";

#[tokio::test]
async fn bundled_bytes_use_cas_and_record_provenance_without_http() {
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let request = DownloadRequest::new(
        "test.bundled.2020",
        Url::parse("https://example.invalid/indices.csv").unwrap(),
        "indices.csv",
    )
    .unwrap()
    .with_context(SourceContext {
        provider: "test-provider".to_owned(),
        dataset: "indices".to_owned(),
        year: 2020,
        month: None,
        edition: "rolling".to_owned(),
        priority: 100,
    });

    let first = store
        .downloads()
        .unwrap()
        .ingest_bundled(&request, BODY.as_bytes(), 1_720_441_696_000)
        .await
        .unwrap();
    let second = store
        .downloads()
        .unwrap()
        .ingest_bundled(&request, BODY.as_bytes(), 1_720_441_696_000)
        .await
        .unwrap();

    assert_eq!(first.disposition, DownloadDisposition::Downloaded);
    assert_eq!(second.disposition, DownloadDisposition::Reused);
    assert_eq!(std::fs::read(first.artifact.path).unwrap(), BODY.as_bytes());
    assert_eq!(
        first.artifact.source_modified_at_utc_ms,
        Some(1_720_441_696_000)
    );
    assert_eq!(
        store
            .artifacts_for_year("test-provider", "indices", 2020)
            .await
            .unwrap()
            .len(),
        1
    );
    let history = store
        .downloads()
        .unwrap()
        .history("test.bundled.2020")
        .await
        .unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.iter().all(|item| item.http_status.is_none()));
}

#[tokio::test]
async fn metadata_query_records_availability_without_getting_a_body() {
    let (url, requests, server) = spawn_server(vec![head_response(BODY.len(), None, None)]);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.query", url, "indices.csv").unwrap();

    let outcome = downloads.query(&request).await.unwrap();
    assert_eq!(outcome.state, RemoteState::Available);
    assert!(
        requests
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("HEAD ")
    );
    let history = downloads.history("test.query").await.unwrap();
    assert_eq!(history[0].status, ObservationStatus::Available);
    assert_eq!(history[0].bytes_received, Some(0));
    assert_eq!(store.layout().objects().read_dir().unwrap().count(), 0);
    server.join().unwrap();
}

#[tokio::test]
async fn downloaded_object_maps_back_to_its_source_partition_and_filename() {
    let (url, _requests, server) = spawn_server(vec![ok_response(BODY, true)]);
    let expected_url = url.to_string();
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let request = DownloadRequest::new("test.origin.2020", url, "indices-2020.csv")
        .unwrap()
        .with_context(SourceContext {
            provider: "test-provider".to_owned(),
            dataset: "indices".to_owned(),
            year: 2020,
            month: None,
            edition: "final".to_owned(),
            priority: 300,
        });

    let outcome = store.downloads().unwrap().download(&request).await.unwrap();
    let origins = store
        .artifact_origins(outcome.artifact.digest)
        .await
        .unwrap();
    assert_eq!(origins.len(), 1);
    assert_eq!(origins[0].canonical_url, expected_url);
    assert_eq!(origins[0].original_filename, "indices-2020.csv");

    let artifacts = store
        .artifacts_for_year("test-provider", "indices", 2020)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].artifact.digest, outcome.artifact.digest);
    assert_eq!(artifacts[0].edition, "final");
    server.join().unwrap();
}

#[tokio::test]
async fn conditional_request_skips_unchanged_remote_body() {
    let responses = vec![ok_response(BODY, true), not_modified_response()];
    let (url, requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.indices", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    let second = downloads.download(&request).await.unwrap();

    assert_eq!(first.disposition, DownloadDisposition::Downloaded);
    assert_eq!(second.disposition, DownloadDisposition::NotModified);
    assert_eq!(first.artifact.digest, second.artifact.digest);
    assert_eq!(first.artifact.path, second.artifact.path);
    assert_eq!(
        first.artifact.downloaded_at_utc_ms,
        second.artifact.downloaded_at_utc_ms
    );
    assert_eq!(
        first.artifact.local_mtime_ns,
        second.artifact.local_mtime_ns
    );
    assert_eq!(
        std::fs::read(&first.artifact.path).unwrap(),
        BODY.as_bytes()
    );
    assert!(
        std::fs::metadata(&first.artifact.path)
            .unwrap()
            .permissions()
            .readonly()
    );
    assert!(first.artifact.downloaded_at_utc_ms > 0);
    assert!(first.artifact.local_mtime_ns > 0);
    assert!(first.artifact.source_modified_at_utc_ms.is_some());

    let first_request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    let second_request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!first_request.to_ascii_lowercase().contains("if-none-match"));
    assert!(
        second_request
            .to_ascii_lowercase()
            .contains("if-none-match: \"v1\"")
    );
    assert!(
        second_request
            .to_ascii_lowercase()
            .contains("if-modified-since: wed, 08 jul 2026 12:34:56 gmt")
    );

    let history = downloads.history("test.indices").await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].status, ObservationStatus::Downloaded);
    assert!(history[0].download_started_at_utc_ms.is_some());
    assert_eq!(
        history[0].bytes_received,
        Some(u64::try_from(BODY.len()).unwrap())
    );
    assert_eq!(history[1].status, ObservationStatus::NotModified);
    assert_eq!(history[1].download_started_at_utc_ms, None);
    assert_eq!(history[1].bytes_received, Some(0));
    assert_eq!(history[1].artifact, Some(first.artifact.digest));
    assert_eq!(history[1].request_etag.as_deref(), Some("\"v1\""));
    assert_eq!(
        history[1].request_last_modified.as_deref(),
        Some(LAST_MODIFIED)
    );
    assert!(history.iter().all(|item| item.queried_at_utc_ms > 0));
    assert!(
        history
            .iter()
            .all(|item| item.download_finished_at_utc_ms.is_some())
    );
    assert_eq!(
        std::fs::read_dir(store.layout().root().join("tmp"))
            .unwrap()
            .count(),
        0
    );

    server.join().unwrap();
}

#[tokio::test]
async fn repeated_full_body_is_deduplicated_by_digest() {
    let responses = vec![
        ok_response(BODY, false),
        head_response(BODY.len(), None, None),
        ok_response(BODY, false),
    ];
    let (url, _requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.no-validators", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    let second = downloads.download(&request).await.unwrap();

    assert_eq!(first.disposition, DownloadDisposition::Downloaded);
    assert_eq!(second.disposition, DownloadDisposition::Reused);
    assert_eq!(first.artifact.path, second.artifact.path);
    assert_eq!(
        first.artifact.downloaded_at_utc_ms,
        second.artifact.downloaded_at_utc_ms
    );
    assert_eq!(
        first.artifact.local_mtime_ns,
        second.artifact.local_mtime_ns
    );
    let history = downloads.history("test.no-validators").await.unwrap();
    assert_eq!(history[0].status, ObservationStatus::Downloaded);
    assert_eq!(history[1].status, ObservationStatus::Unchanged);

    server.join().unwrap();
}

#[tokio::test]
async fn stable_source_identity_survives_an_upstream_url_change() {
    let responses = vec![
        ok_response(BODY, true),
        head_response(BODY.len(), Some("\"v1\""), Some(LAST_MODIFIED)),
        ok_response(BODY, true),
    ];
    let (mut url, _requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    url.set_path("/old-location.csv");
    let first_request = DownloadRequest::new("test.moved", url.clone(), "indices.csv").unwrap();
    let first = downloads.download(&first_request).await.unwrap();

    url.set_path("/new-location.csv");
    let moved_request = DownloadRequest::new("test.moved", url, "indices.csv").unwrap();
    let moved = downloads.download(&moved_request).await.unwrap();

    assert_eq!(moved.disposition, DownloadDisposition::Reused);
    assert_eq!(moved.artifact.digest, first.artifact.digest);
    assert_eq!(downloads.history("test.moved").await.unwrap().len(), 2);
    server.join().unwrap();
}

#[tokio::test]
async fn changed_remote_body_creates_a_new_artifact() {
    let changed = "kp,ap,f107\n4,27,180\n";
    let responses = vec![
        ok_response(BODY, true),
        head_response(
            changed.len(),
            Some("\"v2\""),
            Some("Thu, 09 Jul 2026 12:34:56 GMT"),
        ),
        versioned_response(changed, "\"v2\"", "Thu, 09 Jul 2026 12:34:56 GMT"),
    ];
    let (url, _requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.changed", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    let second = downloads.download(&request).await.unwrap();

    assert_eq!(second.disposition, DownloadDisposition::Downloaded);
    assert_ne!(first.artifact.digest, second.artifact.digest);
    assert_ne!(first.artifact.path, second.artifact.path);
    assert_eq!(
        std::fs::read(&second.artifact.path).unwrap(),
        changed.as_bytes()
    );
    let history = downloads.history("test.changed").await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(
        history
            .iter()
            .all(|item| item.status == ObservationStatus::Downloaded)
    );

    server.join().unwrap();
}

#[tokio::test]
async fn http_failure_is_persisted() {
    let unavailable =
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_owned();
    let (url, _requests, server) =
        spawn_server(vec![unavailable.clone(), unavailable.clone(), unavailable]);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.failure", url, "indices.csv").unwrap();

    assert!(downloads.download(&request).await.is_err());
    let history = downloads.history("test.failure").await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, ObservationStatus::Failed);
    assert_eq!(history[0].http_status, Some(503));
    assert_eq!(history[0].download_started_at_utc_ms, None);
    assert_eq!(history[0].error_kind.as_deref(), Some("http_status"));

    server.join().unwrap();
}

#[tokio::test]
async fn corrupt_local_baseline_is_persisted_and_not_sent() {
    let (url, requests, server) = spawn_server(vec![ok_response(BODY, true)]);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.corrupt", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    make_writable(&first.artifact.path);
    std::fs::write(&first.artifact.path, b"corrupt").unwrap();

    assert!(downloads.download(&request).await.is_err());
    let history = downloads.history("test.corrupt").await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].status, ObservationStatus::Failed);
    assert_eq!(history[1].error_kind.as_deref(), Some("integrity"));
    assert_eq!(history[1].http_status, None);
    assert_eq!(requests.try_iter().count(), 1);

    server.join().unwrap();
}

#[tokio::test]
async fn force_content_repairs_a_corrupt_local_object() {
    let (url, requests, server) =
        spawn_server(vec![ok_response(BODY, true), ok_response(BODY, true)]);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.repair", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    make_writable(&first.artifact.path);
    std::fs::write(&first.artifact.path, b"corrupt").unwrap();

    let repaired = downloads
        .check(&request, CheckMode::ForceContent)
        .await
        .unwrap();
    assert_eq!(repaired.artifact.digest, first.artifact.digest);
    assert_eq!(
        std::fs::read(&repaired.artifact.path).unwrap(),
        BODY.as_bytes()
    );
    assert_eq!(requests.try_iter().count(), 2);
    server.join().unwrap();
}

#[tokio::test]
async fn matching_head_metadata_skips_the_body() {
    let responses = vec![
        ok_response(BODY, true),
        head_response(BODY.len(), Some("\"v1\""), Some(LAST_MODIFIED)),
    ];
    let (url, requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.metadata", url, "indices.csv").unwrap();

    let first = downloads.download(&request).await.unwrap();
    let second = downloads.download(&request).await.unwrap();

    assert_eq!(second.disposition, DownloadDisposition::MetadataUnchanged);
    assert_eq!(first.artifact.digest, second.artifact.digest);
    let history = downloads.history("test.metadata").await.unwrap();
    assert_eq!(history[1].status, ObservationStatus::MetadataUnchanged);
    assert_eq!(history[1].bytes_received, Some(0));
    assert_eq!(history[1].download_started_at_utc_ms, None);
    let methods = requests
        .try_iter()
        .map(|request| request.split_whitespace().next().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(methods, ["GET", "HEAD"]);

    server.join().unwrap();
}

#[tokio::test]
async fn force_content_downloads_and_hashes_the_remote_body() {
    let responses = vec![ok_response(BODY, true), ok_response(BODY, true)];
    let (url, requests, server) = spawn_server(responses);
    let temporary = TempDir::new().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let downloads = store.downloads().unwrap();
    let request = DownloadRequest::new("test.force", url, "indices.csv").unwrap();

    downloads.download(&request).await.unwrap();
    let forced = downloads
        .check(&request, CheckMode::ForceContent)
        .await
        .unwrap();

    assert_eq!(forced.disposition, DownloadDisposition::Reused);
    let sent = requests.try_iter().collect::<Vec<_>>();
    assert!(sent.iter().all(|request| request.starts_with("GET ")));
    assert!(!sent[1].to_ascii_lowercase().contains("if-none-match"));

    server.join().unwrap();
}

fn make_writable(path: &std::path::Path) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(not(unix))]
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn ok_response(body: &str, validators: bool) -> String {
    if validators {
        versioned_response(body, "\"v1\"", LAST_MODIFIED)
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/csv\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }
}

fn versioned_response(body: &str, etag: &str, modified: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/csv\r\nETag: {etag}\r\nLast-Modified: {modified}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn not_modified_response() -> String {
    format!(
        "HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\nLast-Modified: {LAST_MODIFIED}\r\nConnection: close\r\n\r\n"
    )
}

fn head_response(size: usize, etag: Option<&str>, modified: Option<&str>) -> String {
    let mut headers = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n");
    if let Some(etag) = etag {
        write!(headers, "ETag: {etag}\r\n").unwrap();
    }
    if let Some(modified) = modified {
        write!(headers, "Last-Modified: {modified}\r\n").unwrap();
    }
    headers.push_str("Connection: close\r\n\r\n");
    headers
}

fn spawn_server(responses: Vec<String>) -> (Url, Receiver<String>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            sender.send(String::from_utf8(request).unwrap()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        }
    });
    (
        Url::parse(&format!("http://{address}/indices.csv")).unwrap(),
        receiver,
        handle,
    )
}
