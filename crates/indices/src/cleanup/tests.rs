use super::*;
use crate::database::{open_existing_year, open_year};
use crate::{
    IndexEdition, IndexFileOrigin, ValidatedIndexFile,
    import::import_parsed_file,
    parse::{HourlyRecord, ParsedFile},
};
use ionoray_store::{ArtifactRef, DownloadDisposition, DownloadOutcome};

fn file(digest: u8) -> ValidatedIndexFile {
    ValidatedIndexFile {
        source_id: format!("source-{digest}"),
        canonical_url: None,
        dataset: IndexDataset::Dst,
        year: 2020,
        month: Some(1),
        edition: IndexEdition::Final,
        records: 1,
        origin: IndexFileOrigin::LocalCas,
        download: DownloadOutcome {
            observation_id: format!("obs-{digest}"),
            disposition: DownloadDisposition::Downloaded,
            artifact: ArtifactRef {
                digest: Sha256Digest::from_bytes([digest; 32]),
                byte_size: 1,
                path: "/fixture".into(),
                original_filename: "fixture".into(),
                media_type: None,
                source_modified_at_utc_ms: None,
                downloaded_at_utc_ms: 1,
                local_mtime_ns: 1,
            },
            queried_at_utc_ms: 1,
            finished_at_utc_ms: 1,
        },
    }
}
fn parsed(values: &[(i64, f64)]) -> ParsedFile {
    ParsedFile::Dst(
        values
            .iter()
            .map(|(epoch_ms, value)| HourlyRecord {
                epoch_ms: *epoch_ms,
                value: Some(*value),
            })
            .collect(),
    )
}
use ionoray_store::{Store, StoreScope};
use tempfile::TempDir;
use turso::params;

#[tokio::test]
async fn keeps_cross_year_current_history_and_selected_source_references() {
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let scoped = root.scoped(StoreScope::new("dst").unwrap()).await.unwrap();
    let guard = scoped.try_acquire_sync_lock().unwrap();
    let digests = [1_u8, 2, 3, 4, 5].map(|n| Sha256Digest::from_bytes([n; 32]));
    for year in [2020, 2021] {
        let database = open_year(
            root.layout().index_database("wdc-kyoto", "dst", year),
            IndexDataset::Dst,
            year,
        )
        .await
        .unwrap();
        let connection = database.database.connect().unwrap();
        let releases = if year == 2020 {
            vec![
                ("current", digests[0]),
                ("history", digests[1]),
                ("source", digests[2]),
                ("old", digests[3]),
            ]
        } else {
            vec![("current", digests[4])]
        };
        for (id, digest) in releases {
            connection.execute("INSERT INTO dataset_release (release_id, artifact_sha256, source_id, edition, edition_priority, parser_name, parser_version, coverage_start_utc_ms, coverage_end_utc_ms, fetched_at_utc_ms, imported_at_utc_ms, record_count, state) VALUES (?, ?, 'test', 'final', 300, 'test', '1', 0, 1, 0, 0, 1, 'ready')", params![id, digest.to_string()]).await.unwrap();
            connection
                .execute("INSERT INTO dst_hourly VALUES (?, 0, 12.0, 'final')", [id])
                .await
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO active_release VALUES ('month-01', 'current')",
                (),
            )
            .await
            .unwrap();
        if year == 2020 {
            connection
                .execute(
                    "INSERT INTO retained_history_release VALUES ('month-01', 'history')",
                    (),
                )
                .await
                .unwrap();
            connection
                .execute(
                    "INSERT INTO sample_provenance VALUES ('current', 'dst_hourly/0', 'source')",
                    (),
                )
                .await
                .unwrap();
        }
    }
    let retained = retain_snapshots(&root, &scoped, IndexDataset::Dst, &guard)
        .await
        .unwrap();
    assert_eq!(
        retained,
        HashSet::from([digests[0], digests[1], digests[2], digests[4]])
    );
    let database = open_existing_year(
        root.layout().index_database("wdc-kyoto", "dst", 2020),
        IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    let connection = database.database.connect().unwrap();
    let mut rows = connection
        .query("SELECT COUNT(*) FROM dst_hourly", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        2
    );
    let mut rows = connection
        .query(
            "SELECT state FROM dataset_release WHERE release_id = 'old'",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "superseded"
    );
}

#[tokio::test]
async fn cleanup_keeps_e_and_d_after_a_b_c_d_e_imports() {
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let scoped = root.scoped(StoreScope::new("dst").unwrap()).await.unwrap();
    let guard = scoped.try_acquire_sync_lock().unwrap();
    let database = open_year(
        root.layout().index_database("wdc-kyoto", "dst", 2020),
        IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    for (digest, records) in [
        (10, vec![(0, 1.0)]),
        (11, vec![(0, 1.0), (1, 2.0)]),
        (12, vec![(0, 3.0), (1, 2.0)]),
        (13, vec![(0, 3.0), (1, 2.0), (2, 4.0)]),
        (14, vec![(0, 5.0), (1, 2.0), (2, 4.0)]),
    ] {
        import_parsed_file(&database, &file(digest), parsed(&records))
            .await
            .unwrap();
    }
    let retained = retain_snapshots(&root, &scoped, IndexDataset::Dst, &guard)
        .await
        .unwrap();
    assert_eq!(
        retained,
        HashSet::from([
            Sha256Digest::from_bytes([13; 32]),
            Sha256Digest::from_bytes([14; 32])
        ])
    );
    let connection = database.database.connect().unwrap();
    let mut rows = connection
        .query("SELECT COUNT(*) FROM dst_hourly", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        6
    );
    let mut rows=connection.query("SELECT a.artifact_sha256,h.artifact_sha256 FROM active_release x JOIN dataset_release a ON a.release_id=x.release_id JOIN retained_history_release y ON y.partition_key=x.partition_key JOIN dataset_release h ON h.release_id=y.release_id",()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(
        row.get::<String>(0).unwrap(),
        Sha256Digest::from_bytes([14; 32]).to_string()
    );
    assert_eq!(
        row.get::<String>(1).unwrap(),
        Sha256Digest::from_bytes([13; 32]).to_string()
    );
}

#[tokio::test]
async fn reimports_discarded_snapshot_after_cleanup() {
    let temporary = TempDir::new().unwrap();
    let root = Store::open_root(Some(temporary.path())).unwrap();
    let scoped = root.scoped(StoreScope::new("dst").unwrap()).await.unwrap();
    let guard = scoped.try_acquire_sync_lock().unwrap();
    let database = open_year(
        root.layout().index_database("wdc-kyoto", "dst", 2020),
        IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    for (digest, value) in [(20, 1.0), (21, 2.0), (22, 3.0)] {
        import_parsed_file(&database, &file(digest), parsed(&[(0, value)]))
            .await
            .unwrap();
    }
    let retained = retain_snapshots(&root, &scoped, IndexDataset::Dst, &guard)
        .await
        .unwrap();
    assert!(!retained.contains(&Sha256Digest::from_bytes([20; 32])));
    import_parsed_file(&database, &file(20), parsed(&[(0, 1.0)]))
        .await
        .unwrap();
    retain_snapshots(&root, &scoped, IndexDataset::Dst, &guard)
        .await
        .unwrap();
    let connection = database.database.connect().unwrap();
    let mut rows = connection.query("SELECT d.dst_nt, source.artifact_sha256 FROM dst_hourly d JOIN active_release active ON active.release_id=d.release_id JOIN sample_provenance p ON p.release_id=d.release_id AND p.sample_key='dst_hourly/' || d.epoch_utc_ms JOIN dataset_release source ON source.release_id=p.source_release_id", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert!((row.get::<f64>(0).unwrap() - 1.0).abs() < f64::EPSILON);
    assert_eq!(
        row.get::<String>(1).unwrap(),
        Sha256Digest::from_bytes([20; 32]).to_string()
    );
    assert!(rows.next().await.unwrap().is_none());
}
