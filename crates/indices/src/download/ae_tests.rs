//! Real Kyoto daily files served locally to exercise the production pipeline.
use super::*;
use crate::{IndexDataset, IndexEdition, IndexFileOrigin, QualityFlag};
use ionoray_store::{DownloadDisposition, Store};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const REALTIME: &str = include_str!("../../tests/fixtures/ae/ae240501.txt");
const PROVISIONAL: &str = include_str!("../../tests/fixtures/ae/ae240510.txt");

struct Server {
    base: String,
    gets: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let gets = Arc::new(AtomicUsize::new(0));
        let count = gets.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut buffer = [0; 1024];
                    let length = stream.read(&mut buffer).await.unwrap();
                    if length == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..length]);
                }
                let request = String::from_utf8(request).unwrap();
                let mut words = request.split_whitespace();
                let method = words.next().unwrap();
                let path = words.next().unwrap();
                let body = match path {
                    "/realtime/1" => Some(REALTIME),
                    "/provisional/10" => Some(PROVISIONAL),
                    "/invalid" => Some("invalid E02"),
                    _ => None,
                };
                let status = if body.is_some() {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let body = body.unwrap_or_default();
                let mut response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if method == "GET" {
                    if !body.is_empty() {
                        count.fetch_add(1, Ordering::Relaxed);
                    }
                    response.push_str(body);
                }
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self { base, gets, task }
    }
}

fn day_source(day: u8, base: &str) -> SourceFile {
    let start = crate::parse::epoch_ms(2024, 5, day, 0).unwrap();
    let mut source = files_for_dataset_interval(IndexDataset::Ae, start, start + 1)
        .unwrap()
        .remove(0);
    source.candidates[0].url = format!("{base}/provisional/{day}");
    source.candidates[1].url = format!("{base}/realtime/{day}");
    source
}

#[tokio::test]
async fn daily_fallback_import_repeat_and_offline_rebuild() {
    let server = Server::start().await;
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let report = download_interval_sources(
        &root,
        vec![day_source(1, &server.base), day_source(10, &server.base)],
        CheckMode::Metadata,
        &[2024],
    )
    .await
    .unwrap();
    assert!(report.failures.is_empty());
    assert_eq!(report.files.len(), 2);
    assert_eq!(report.files[0].file.edition, IndexEdition::Realtime);
    assert_eq!(report.files[1].file.edition, IndexEdition::Provisional);
    let database = crate::maintenance::open_dataset(&root, IndexDataset::Ae, 2024, false)
        .await
        .unwrap();
    for file in report.files {
        assert_eq!(file.file.records, 24);
        crate::import::import_parsed_file(&database, &file.file, file.parsed)
            .await
            .unwrap();
    }
    assert_values(&database.database).await;
    let gets = server.gets.load(Ordering::Relaxed);
    let repeat = download_file(&root, day_source(1, &server.base), CheckMode::Metadata)
        .await
        .unwrap();
    assert_eq!(server.gets.load(Ordering::Relaxed), gets);
    assert_eq!(
        repeat.files[0].file.download.disposition,
        DownloadDisposition::MetadataUnchanged
    );
    for file in repeat.files {
        let summary = crate::import::import_parsed_file(&database, &file.file, file.parsed)
            .await
            .unwrap();
        assert_eq!(summary.releases_created, 0);
        assert_eq!(summary.releases_reused, 1);
    }
    let scoped = scoped(&root, IndexDataset::Ae).await.unwrap();
    let artifacts = scoped
        .accepted_artifacts_for_year("wdc-kyoto", "ae", 2024)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 2);
    assert!(artifacts.iter().any(|artifact| artifact.day == Some(1)));
    assert!(artifacts.iter().any(|artifact| artifact.day == Some(10)));
    assert_eq!(
        scoped
            .artifacts_for_year("wdc-kyoto", "ae", 2024)
            .await
            .unwrap()
            .len(),
        2
    );
    // Pruning must retain both daily bodies, including their sample provenance.
    let guard = scoped.try_acquire_sync_lock().unwrap();
    let retained = crate::cleanup::retain_snapshots(&root, &scoped, IndexDataset::Ae, &guard)
        .await
        .unwrap();
    assert_eq!(retained.len(), 2);
    drop(guard);
    drop(database);
    drop(scoped);
    drop(server); // No HTTP server exists during recovery.
    let path = root.layout().index_database("wdc-kyoto", "ae", 2024);
    std::fs::remove_file(path).unwrap();
    let start = crate::parse::epoch_ms(2024, 5, 1, 0).unwrap();
    let end = crate::parse::epoch_ms(2024, 5, 11, 0).unwrap();
    let recovered = crate::maintenance::sync_dataset_interval(
        &root,
        IndexDataset::Ae,
        start,
        end,
        crate::SyncPolicy::Offline,
    )
    .await
    .unwrap();
    assert!(recovered.failures.is_empty());
    assert_eq!(recovered.report.records_imported, 48);
    assert!(
        recovered
            .files
            .iter()
            .all(|file| file.origin == IndexFileOrigin::LocalCas)
    );
    let database = crate::maintenance::open_dataset(&root, IndexDataset::Ae, 2024, true)
        .await
        .unwrap();
    assert_values(&database.database).await;
}

async fn assert_values(database: &turso::Database) {
    for (day, value, quality) in [
        (1, 647.0, QualityFlag::Realtime),
        (10, 168.0, QualityFlag::Provisional),
    ] {
        let epoch =
            ionoray_core::Epoch::maybe_from_gregorian_utc(2024, 5, day, 0, 0, 0, 0).unwrap();
        let sample = crate::query::read_ae(database, epoch).await.unwrap();
        assert!((sample.value.value() - value).abs() < f64::EPSILON);
        assert_eq!(sample.quality, quality);
    }
}

#[tokio::test]
async fn unavailable_day_retains_both_urls_and_exact_date() {
    let server = Server::start().await;
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let report = download_file(&root, day_source(9, &server.base), CheckMode::Metadata)
        .await
        .unwrap();
    assert!(report.files.is_empty());
    assert_eq!(report.failures.len(), 2);
    for failure in report.failures {
        assert_eq!(failure.day, Some(9));
        assert_eq!(failure.kind, IntervalFailureKind::RemoteUnavailable);
        assert!(failure.diagnostic().contains("2024-05-09 http://"));
        assert!(failure.message.contains("404"));
    }
}

#[tokio::test]
async fn malformed_higher_edition_is_reported_even_when_fallback_succeeds() {
    let server = Server::start().await;
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let mut source = day_source(1, &server.base);
    source.candidates[0].url = format!("{}/invalid", server.base);
    let report = download_file(&root, source, CheckMode::Metadata)
        .await
        .unwrap();
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].kind, IntervalFailureKind::Parse);
}

#[tokio::test]
async fn rejects_wrong_day_and_duplicate_hour_and_preserves_missing_mean() {
    let temporary = TempDir::new().unwrap();
    let path = temporary.path().join("ae");
    tokio::fs::write(&path, REALTIME).await.unwrap();
    assert!(
        crate::parse::parse_file(IndexDataset::Ae, &path, 2024, Some(5), Some(2))
            .await
            .is_err()
    );
    let first = REALTIME.lines().next().unwrap();
    tokio::fs::write(&path, format!("{REALTIME}\n{first}\n"))
        .await
        .unwrap();
    assert!(
        crate::parse::parse_file(IndexDataset::Ae, &path, 2024, Some(5), Some(1))
            .await
            .is_err()
    );
    // Explicit synthetic boundary perturbation of a real fixture: declared missing mean.
    let mut missing = REALTIME.as_bytes().to_vec();
    missing[394..400].copy_from_slice(b" 99999");
    tokio::fs::write(&path, missing).await.unwrap();
    let parsed = crate::parse::parse_file(IndexDataset::Ae, &path, 2024, Some(5), Some(1))
        .await
        .unwrap();
    let ParsedFile::Ae(records) = parsed else {
        panic!("expected AE")
    };
    assert_eq!(records[0].value, None);
    assert_eq!(records.len(), 24);
}
