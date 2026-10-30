//! Local HTTP fixtures verify warnings with debug spans filtered out.
use std::{
    io::Write,
    sync::{Arc, Mutex},
};
use tracing::instrument::WithSubscriber;

use super::{DownloadError, DownloadRequest, SourceContext, contract_tests::server};
use crate::Store;

#[derive(Clone)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn missing_source_warning(context: Option<SourceContext>) -> String {
    let (url, _requests, join) = server(vec![
        "HTTP/1.1 404 Not Found\r\nContent-Length:0\r\nConnection: close\r\n\r\n".into(),
    ]);
    let temporary = tempfile::tempdir().unwrap();
    let store = Store::open(Some(temporary.path())).await.unwrap();
    let mut request = DownloadRequest::new("test.missing", url.clone(), "missing.txt").unwrap();
    if let Some(context) = context {
        request = request.with_context(context);
    }
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer = LogBuffer(bytes.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let downloads = store.downloads().unwrap();
    let error = downloads
        .download(&request)
        .with_subscriber(subscriber)
        .await
        .unwrap_err();
    assert!(matches!(error, DownloadError::UnexpectedStatus(404)));
    join.join().unwrap();
    let log = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    let warnings: Vec<_> = log
        .lines()
        .filter(|line| line.contains("download check failed"))
        .collect();
    assert_eq!(warnings.len(), 1, "{log}");
    let warning = warnings[0];
    for field in [
        "WARN",
        "source_id=\"test.missing\"",
        "logical_name=\"missing.txt\"",
        "HTTP status 404",
        url.as_str(),
    ] {
        assert!(warning.contains(field), "missing {field}: {warning}");
    }
    assert!(
        !warning.contains("download.check{"),
        "debug span should be filtered: {warning}"
    );
    warning.to_owned()
}

#[tokio::test]
async fn warning_contains_source_context_at_info_level() {
    let warning = missing_source_warning(Some(SourceContext {
        provider: "wdc-kyoto".into(),
        dataset: "dst".into(),
        year: 2025,
        month: Some(1),
        day: None,
        edition: "provisional".into(),
        priority: 10,
    }))
    .await;
    for field in [
        "provider=\"wdc-kyoto\"",
        "dataset=\"dst\"",
        "year=2025",
        "month=1",
        "edition=\"provisional\"",
    ] {
        assert!(warning.contains(field), "missing {field}: {warning}");
    }
    assert!(!warning.contains("day="));
}

#[tokio::test]
async fn warning_without_context_keeps_url_and_source_id() {
    let warning = missing_source_warning(None).await;
    for field in [
        "provider=",
        "dataset=",
        "year=",
        "month=",
        "day=",
        "edition=",
    ] {
        assert!(!warning.contains(field), "invented {field}: {warning}");
    }
}
