use super::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(responses: Vec<(u16, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        let mut responses = VecDeque::from(responses);
        let task = tokio::spawn(async move {
            while let Some((status, body)) = responses.pop_front() {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = Vec::new();
                let mut byte = [0];
                while !data.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).await.unwrap();
                    data.push(byte[0]);
                }
                let line = String::from_utf8(data)
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                let head = line.starts_with("HEAD");
                seen.lock().unwrap().push(line);
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                if !head {
                    socket.write_all(&body).await.unwrap();
                }
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }
}

fn slots(dataset: ActivityDataset, range: ActivityRange, base: &str) -> Vec<source::Slot> {
    let mut slots = source::slots(dataset, range).unwrap();
    for (_, candidates) in &mut slots {
        for candidate in candidates {
            candidate.url = url::Url::parse(&format!("{base}{}", candidate.url.path())).unwrap();
        }
    }
    slots
}

#[tokio::test]
async fn rolling_file_dedup_revision_failure_and_offline_read() {
    let original = include_bytes!("../../tests/fixtures/activity/hp30.txt").to_vec();
    // Synthetic metadata-only revision over unchanged real observed samples.
    let mut revision = original.clone();
    revision.extend_from_slice(b"# Metadata revision test\n");
    // Synthetic transport truncation: remove actual trailing records.
    let truncated = original
        .split(|b| *b == b'\n')
        .take(31)
        .flat_map(|s| s.iter().copied().chain(*b"\n"))
        .collect();
    let server = Server::start(vec![
        (200, original.clone()),
        (200, original),
        (200, revision),
        (200, truncated),
        (200, b"invalid".to_vec()),
    ])
    .await;
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2024-05-10", "2024-05-12").unwrap();
    let sources = slots(ActivityDataset::Hp30, range, &server.base);
    let first = store
        .download_slots(ActivityDataset::Hp30, range, false, sources.clone())
        .await
        .unwrap();
    assert_eq!(first.files.len(), 1);
    assert_eq!(first.coverage.expected_records, 96);
    assert!(!first.coverage.missing_time_ranges_utc_ms.is_empty());
    assert_eq!(first.files[0].change, Some("first_acquired"));
    let repeated = store
        .download_slots(ActivityDataset::Hp30, range, false, sources.clone())
        .await
        .unwrap();
    assert_eq!(repeated.files[0].change, Some("unchanged"));
    assert_eq!(server.requests.lock().unwrap().len(), 2); // One GET, one HEAD; not one per day.
    let revised = store
        .download_slots(ActivityDataset::Hp30, range, true, sources.clone())
        .await
        .unwrap();
    assert_eq!(revised.files[0].change, Some("revised"));
    assert!(first.files[0].artifact.path.is_file());
    assert!(
        store
            .download_slots(ActivityDataset::Hp30, range, true, sources.clone())
            .await
            .is_err()
    );
    assert!(
        store
            .download_slots(ActivityDataset::Hp30, range, true, sources)
            .await
            .is_err()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 5);
    drop(server);
    let mut rows = Vec::new();
    let read = store
        .read(ActivityDataset::Hp30, range, |row| {
            rows.push(row.clone());
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        read.files[0].artifact.digest,
        revised.files[0].artifact.digest
    );
    assert_eq!(read.files[0].source_check, "offline_not_checked");
    assert_eq!(rows[0].utc, "2024-05-10T00:00:00+00:00");
    assert_eq!(rows[0].end_utc, "2024-05-10T00:30:00+00:00");
    assert_eq!(rows[0].units, "1");
    let object = &revised.files[0].artifact.path;
    std::fs::remove_file(object).unwrap();
    std::fs::write(object, b"corrupt").unwrap();
    assert!(
        store
            .read(ActivityDataset::Hp30, range, |_| Ok(()))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn pcn_fallback_retention_and_server_failure_do_not_invent_final_values() {
    let body = include_bytes!("../../tests/fixtures/activity/pcn-provisional.csv").to_vec();
    let server = Server::start(vec![
        (404, vec![]),
        (200, body),
        (404, vec![]),
        (404, vec![]),
        (503, vec![]),
        (503, vec![]),
        (503, vec![]),
    ])
    .await;
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2026-01-01T00:00:00Z", "2026-01-01T00:04:00Z").unwrap();
    let sources = slots(ActivityDataset::Pcn, range, &server.base);
    let first = store
        .download_slots(ActivityDataset::Pcn, range, false, sources.clone())
        .await
        .unwrap();
    assert_eq!(first.files[0].edition, "provisional");
    assert_eq!(first.coverage.records, 4);
    assert_eq!(first.unavailable_sources.len(), 1);
    let retained = store
        .download_slots(ActivityDataset::Pcn, range, false, sources.clone())
        .await
        .unwrap();
    assert_eq!(retained.files[0].source_check, "upstream_missing");
    assert!(
        store
            .download_slots(ActivityDataset::Pcn, range, false, sources)
            .await
            .is_err()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 7);
    drop(server);
    let report = store
        .read(ActivityDataset::Pcn, range, |row| {
            assert_eq!(row.quality, "provisional");
            assert_eq!(row.cadence_seconds, 60);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        report.files[0].artifact.digest,
        first.files[0].artifact.digest
    );
}

#[tokio::test]
async fn missing_offline_sources_fail_without_output() {
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2024-05-10", "2024-05-11").unwrap();
    let mut count = 0;
    assert!(matches!(
        store
            .read(ActivityDataset::Pcn, range, |_| {
                count += 1;
                Ok(())
            })
            .await,
        Err(ActivityError::Missing { .. })
    ));
    assert_eq!(count, 0);
    let plan = source::slots(ActivityDataset::Pcn, range).unwrap();
    let priorities: Vec<_> = plan[0].1.iter().map(|s| s.priority).collect();
    assert_eq!(priorities, [300, 200, 100]);
}

#[tokio::test]
async fn wrong_pcn_calendar_is_not_accepted_as_another_year() {
    let server = Server::start(vec![(
        200,
        include_bytes!("../../tests/fixtures/activity/pcn-iaga.txt").to_vec(),
    )])
    .await;
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2026-01-01", "2026-01-02").unwrap();
    let sources = slots(ActivityDataset::Pcn, range, &server.base);
    assert!(
        store
            .download_slots(ActivityDataset::Pcn, range, false, sources)
            .await
            .is_err()
    );
    assert!(
        store
            .read(ActivityDataset::Pcn, range, |_| Ok(()))
            .await
            .is_err()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn real_quicklook_csv_is_used_only_after_both_higher_editions_are_absent() {
    let server = Server::start(vec![
        (404, vec![]),
        (404, vec![]),
        (
            200,
            include_bytes!("../../tests/fixtures/activity/pcn-quicklook.csv").to_vec(),
        ),
    ])
    .await;
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2026-10-28T00:00:00Z", "2026-10-28T00:04:00Z").unwrap();
    let sources = slots(ActivityDataset::Pcn, range, &server.base);
    let report = store
        .download_slots(ActivityDataset::Pcn, range, false, sources)
        .await
        .unwrap();
    assert_eq!(report.files[0].edition, "quicklook");
    assert_eq!(report.unavailable_sources.len(), 2);
    assert_eq!(report.coverage.records, 4);
    assert!(server.requests.lock().unwrap()[2].contains("PCN_quicklook/pcn20261028q.csv"));
    drop(server);
    store
        .read(ActivityDataset::Pcn, range, |row| {
            assert_eq!(row.quality, "quicklook");
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn accepted_definitive_is_not_downgraded_when_upstream_is_absent() {
    let server = Server::start(vec![
        (
            200,
            include_bytes!("../../tests/fixtures/activity/pcn-iaga.txt").to_vec(),
        ),
        (404, vec![]),
    ])
    .await;
    let home = tempfile::tempdir().unwrap();
    let store = ActivityStore::open(Some(home.path())).unwrap();
    let range = ActivityRange::parse("2024-05-10T00:00:00Z", "2024-05-10T00:02:00Z").unwrap();
    let sources = slots(ActivityDataset::Pcn, range, &server.base);
    let first = store
        .download_slots(ActivityDataset::Pcn, range, false, sources.clone())
        .await
        .unwrap();
    let retained = store
        .download_slots(ActivityDataset::Pcn, range, false, sources)
        .await
        .unwrap();
    assert_eq!(retained.files[0].edition, "definitive");
    assert_eq!(retained.files[0].source_check, "upstream_missing");
    assert_eq!(
        retained.files[0].artifact.digest,
        first.files[0].artifact.digest
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}
