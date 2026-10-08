use crate::{CheckMode, DownloadRequest, Store, StoreScope};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::TempDir;
use url::Url;

fn server(responses: Vec<String>) -> (Url, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for response in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 1024];
            while !bytes.windows(4).any(|x| x == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            tx.send(String::from_utf8(bytes).unwrap()).unwrap();
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (
        Url::parse(&format!("http://{address}/x")).unwrap(),
        rx,
        handle,
    )
}
fn ok(body: &str, etag: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {etag}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn head_405_falls_back_to_get() {
    let (url, rx, join) = server(vec![
        ok("A", "\"a\""),
        "HTTP/1.1 405 Method Not Allowed\r\nContent-Length:0\r\nConnection: close\r\n\r\n".into(),
        ok("A", "\"a\""),
    ]);
    let temp = TempDir::new().unwrap();
    let store = Store::open(Some(temp.path())).await.unwrap();
    let request = DownloadRequest::new("x", url, "x").unwrap();
    store.downloads().unwrap().download(&request).await.unwrap();
    store.downloads().unwrap().download(&request).await.unwrap();
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("GET ")
    );
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("HEAD ")
    );
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("GET ")
    );
    join.join().unwrap();
}

#[tokio::test]
async fn retry_503_is_bounded_then_recovers() {
    let (url, rx, join) = server(vec![
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length:0\r\nConnection: close\r\n\r\n".into(),
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length:0\r\nConnection: close\r\n\r\n".into(),
        ok("A", "\"a\""),
    ]);
    let temp = TempDir::new().unwrap();
    let store = Store::open(Some(temp.path())).await.unwrap();
    store
        .downloads()
        .unwrap()
        .download(&DownloadRequest::new("retry", url, "x").unwrap())
        .await
        .unwrap();
    for _ in 0..3 {
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .unwrap()
                .starts_with("GET ")
        );
    }
    join.join().unwrap();
}

#[tokio::test]
async fn retry_503_stops_after_three_attempts() {
    let unavailable =
        "HTTP/1.1 503 Service Unavailable\r\nContent-Length:0\r\nConnection: close\r\n\r\n"
            .to_owned();
    let (url, rx, join) = server(vec![unavailable.clone(), unavailable.clone(), unavailable]);
    let temp = TempDir::new().unwrap();
    let store = Store::open(Some(temp.path())).await.unwrap();
    assert!(
        store
            .downloads()
            .unwrap()
            .download(&DownloadRequest::new("fail", url, "x").unwrap())
            .await
            .is_err()
    );
    for _ in 0..3 {
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .unwrap()
                .starts_with("GET ")
        );
    }
    join.join().unwrap();
}

#[tokio::test]
async fn unaccepted_candidate_does_not_replace_scoped_etag_baseline() {
    let head = "HTTP/1.1 304 Not Modified\r\nETag: \"a\"\r\nConnection: close\r\n\r\n".to_owned();
    let (url, rx, join) = server(vec![ok("A", "\"a\""), ok("B", "\"b\""), head]);
    let temp = TempDir::new().unwrap();
    let scoped = Store::open_scoped(Some(temp.path()), StoreScope::new("dst").unwrap())
        .await
        .unwrap();
    let request = DownloadRequest::new("source", url, "x").unwrap();
    let first = scoped
        .downloads()
        .unwrap()
        .download(&request)
        .await
        .unwrap();
    scoped.accept(&request, &first.artifact).await.unwrap();
    scoped
        .downloads()
        .unwrap()
        .check(&request, CheckMode::ForceContent)
        .await
        .unwrap();
    scoped
        .downloads()
        .unwrap()
        .download(&request)
        .await
        .unwrap();
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("GET ")
    );
    assert!(
        rx.recv_timeout(Duration::from_secs(1))
            .unwrap()
            .starts_with("GET ")
    );
    let conditional = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(conditional.starts_with("HEAD "));
    assert!(
        conditional.contains("if-none-match: \"a\"")
            || conditional.contains("If-None-Match: \"a\"")
    );
    join.join().unwrap();
}
