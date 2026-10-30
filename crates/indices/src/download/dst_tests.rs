//! Verbatim Kyoto observations served locally; HTTP responses are test fixtures.
use super::*;
use crate::{IndexDataset, IndexEdition, import::import_parsed_file, maintenance::open_dataset};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn dst_provisional_footer_survives_final_404_download_import_and_reuse() {
    let body = include_bytes!("../../tests/fixtures/dst/dst2405.for.request");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for (path, status, bytes) in [("/final", 404, &b""[..]), ("/provisional", 200, &body[..])] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut buffer = [0];
                stream.read_exact(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer);
            }
            assert!(
                String::from_utf8(request)
                    .unwrap()
                    .starts_with(&format!("GET {path} "))
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(bytes).await.unwrap();
        }
    });
    let home = tempfile::tempdir().unwrap();
    let root = ionoray_store::Store::open_root(Some(home.path())).unwrap();
    let source = SourceFile {
        dataset: IndexDataset::Dst,
        year: 2024,
        month: Some(5),
        day: None,
        logical_name: "dst2405.for.request".into(),
        cache: None,
        candidates: [
            ("final", IndexEdition::Final),
            ("provisional", IndexEdition::Provisional),
            ("realtime", IndexEdition::Realtime),
        ]
        .into_iter()
        .map(|(path, edition)| SourceCandidate {
            source_id: format!("test.dst.{path}"),
            edition,
            url: format!("{base}/{path}"),
        })
        .collect(),
    };
    let report = download_file(&root, source, CheckMode::Metadata)
        .await
        .unwrap();
    server.await.unwrap();
    assert!(report.failures.is_empty());
    assert_eq!(report.files.len(), 1);
    let prepared = report.files.into_iter().next().unwrap();
    assert_eq!(prepared.file.edition, IndexEdition::Provisional);
    assert_eq!(prepared.file.records, 744);
    assert_eq!(
        std::fs::read(&prepared.file.download.artifact.path).unwrap(),
        body
    );
    let database = open_dataset(&root, IndexDataset::Dst, 2024, false)
        .await
        .unwrap();
    let first = import_parsed_file(&database, &prepared.file, prepared.parsed.clone())
        .await
        .unwrap();
    assert_eq!(first.records, 744);
    assert_eq!(first.releases_created, 1);
    let repeated = import_parsed_file(&database, &prepared.file, prepared.parsed)
        .await
        .unwrap();
    assert_eq!(repeated.releases_created, 0);
    assert_eq!(repeated.releases_reused, 1);
    drop(database);
    let store = crate::IndexStore::open(Some(home.path())).await.unwrap();
    let sample = store
        .dst_at("2024-05-11T02:00:00 UTC".parse().unwrap())
        .await
        .unwrap();
    assert!((sample.value.value() + 406.0).abs() < f64::EPSILON);
    assert_eq!(sample.quality, crate::QualityFlag::Provisional);
}
