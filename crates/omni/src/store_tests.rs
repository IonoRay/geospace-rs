use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use super::*;

pub(super) struct Server {
    pub(super) base: String,
    paths: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    pub(super) fn start(responses: Vec<(u16, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let paths = Arc::new(Mutex::new(Vec::new()));
        let requests = Arc::clone(&paths);
        let thread = thread::spawn(move || {
            for (status, body) in responses {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() > deadline {
                                return;
                            }
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let line = String::from_utf8(request)
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                let head = line.starts_with("HEAD ");
                requests.lock().unwrap().push(line);
                write!(
                    socket,
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nETag: \"fixture\"\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                if !head {
                    socket.write_all(&body).unwrap();
                }
            }
        });
        Self {
            base,
            paths,
            thread: Some(thread),
        }
    }
    pub(super) fn finish(mut self) -> Vec<String> {
        self.thread.take().unwrap().join().unwrap();
        Arc::try_unwrap(self.paths).unwrap().into_inner().unwrap()
    }
}

pub(super) fn range() -> OmniRange {
    OmniRange::parse("2024-05-10T00:00:00Z", "2024-05-10T00:03:00Z").unwrap()
}

#[tokio::test]
async fn absent_hro2_tries_hro_but_http_failures_stop() {
    for (responses, count) in [
        (
            vec![(404, vec![]), (200, b"Index of /hro_1min/2024/".to_vec())],
            2,
        ),
        (vec![(503, vec![]), (503, vec![]), (503, vec![])], 3),
        (vec![(200, b"<html>bad gateway</html>".to_vec())], 1),
    ] {
        let server = Server::start(responses);
        let home = tempfile::tempdir().unwrap();
        let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
        let error = store.download(range(), None, false).await.unwrap_err();
        if count == 2 {
            assert!(matches!(error, OmniError::MissingMonth { .. }));
        }
        let paths = server.finish();
        assert_eq!(paths.len(), count);
        assert!(paths[0].contains("/hro2_1min/2024/"));
        if count == 2 {
            assert!(paths[1].contains("/hro_1min/2024/"));
        }
        if count == 3 {
            assert!(paths.iter().all(|p| p.contains("/hro2_1min/2024/")));
        }
        assert!(
            home.path()
                .join("omni/objects/omni-1min/catalog.db")
                .is_file()
        );
        assert!(!home.path().join("objects").exists());
    }
}

#[tokio::test]
async fn offline_missing_month_makes_no_http_request() {
    let home = tempfile::tempdir().unwrap();
    let store = OmniStore::open_at(home.path(), "http://127.0.0.1:1/")
        .await
        .unwrap();
    let mut emitted = 0;
    let result = store
        .read(range(), None, |_| {
            emitted += 1;
            Ok(())
        })
        .await;
    assert!(matches!(result, Err(OmniError::MissingMonth { .. })));
    assert_eq!(emitted, 0);
}

#[tokio::test]
async fn malformed_cdf_is_not_accepted_or_replaced_by_hro() {
    let server = Server::start(vec![
        (200, b"Index of omni_hro2_1min_20240501_v01.cdf".to_vec()),
        (200, b"not a cdf".to_vec()),
    ]);
    let home = tempfile::tempdir().unwrap();
    let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
    assert!(matches!(
        store.download(range(), None, false).await,
        Err(OmniError::Cdf(_))
    ));
    assert!(
        store
            .scoped
            .accepted_artifacts_for_year("nasa-spdf", "hro2_1min", 2024)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.finish().len(), 2);
}

#[tokio::test]
#[ignore = "requires official May 2024 HRO and HRO2 CDFs in /tmp or OMNI_TEST_CDF / OMNI_TEST_HRO_CDF"]
async fn real_cdf_download_fallback_and_offline_read() {
    for product in [OmniProduct::Hro2, OmniProduct::Hro] {
        let (env, default) = if product == OmniProduct::Hro2 {
            ("OMNI_TEST_CDF", "/tmp/geospace-omni-hro2-202405.cdf")
        } else {
            ("OMNI_TEST_HRO_CDF", "/tmp/geospace-omni-hro-202405.cdf")
        };
        let path = std::env::var_os(env).map_or_else(|| default.into(), PathBuf::from);
        if !path.try_exists().unwrap() {
            eprintln!("not executed for {product:?}: missing {}", path.display());
            continue;
        }
        let body = fs::read(&path).unwrap();
        let filename = format!("omni_{}_20240501_v01.cdf", product.dataset());
        let mut responses = Vec::new();
        if product == OmniProduct::Hro {
            responses.push((404, vec![]));
        }
        responses.push((200, format!("Index of {filename}").into_bytes()));
        responses.push((200, body));
        let server = Server::start(responses);
        let home = tempfile::tempdir().unwrap();
        let store = OmniStore::open_at(home.path(), &server.base).await.unwrap();
        let downloaded = store.download(range(), None, false).await.unwrap();
        assert_eq!(downloaded.files[0].identity.product, product);
        assert_eq!(downloaded.files[0].records, 44_640);
        assert_eq!(
            downloaded.unavailable.len(),
            usize::from(product == OmniProduct::Hro)
        );
        server.finish(); // Server is gone: the following read must stay offline.
        let mut rows = Vec::new();
        let report = store
            .read(range(), None, |row| {
                rows.push(serde_json::to_value(row).unwrap());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(report.coverage.expected_records, 3);
        assert_eq!(report.coverage.records, 3);
        assert!(report.coverage.missing_time_ranges_utc_ms.is_empty());
        assert_eq!(rows[0]["record_index"], 9 * 24 * 60);
        assert_eq!(rows[0]["utc"], "2024-05-10T00:00:00+00:00");
        assert_eq!(
            rows[0]["sha256"],
            downloaded.files[0].artifact.digest.to_string()
        );
        assert!(downloaded.files[0].browse_path.as_ref().unwrap().is_file());
        let partial_minute =
            OmniRange::parse("2024-05-10T00:00:30Z", "2024-05-10T00:01:00Z").unwrap();
        let report = store
            .read(partial_minute, None, |_| {
                panic!("no minute timestamp in interval")
            })
            .await
            .unwrap();
        assert_eq!(report.coverage.expected_records, 0);
        assert!(report.coverage.missing_time_ranges_utc_ms.is_empty());
        let object = &downloaded.files[0].artifact.path;
        fs::remove_file(object).unwrap();
        fs::write(object, b"corrupted").unwrap();
        assert!(store.read(range(), None, |_| Ok(())).await.is_err());
    }
}
