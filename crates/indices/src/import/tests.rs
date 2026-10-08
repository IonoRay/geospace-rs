use super::*;
use crate::IndexEdition;
use ionoray_store::{ArtifactRef, DownloadDisposition, DownloadOutcome};
use tempfile::TempDir;

fn file(digest: u8, month: u8, edition: IndexEdition) -> ValidatedIndexFile {
    file_for_source(digest, month, edition, &format!("source-{digest}"))
}

fn file_for_source(
    digest: u8,
    month: u8,
    edition: IndexEdition,
    source_id: &str,
) -> ValidatedIndexFile {
    ValidatedIndexFile {
        source_id: source_id.to_owned(),
        canonical_url: None,
        dataset: crate::IndexDataset::Dst,
        year: 2020,
        month: Some(month),
        edition,
        records: 1,
        origin: crate::IndexFileOrigin::LocalCas,
        download: DownloadOutcome {
            observation_id: format!("observation-{digest}"),
            disposition: DownloadDisposition::Downloaded,
            artifact: ArtifactRef {
                digest: Sha256Digest::from_bytes([digest; 32]),
                byte_size: 1,
                path: std::path::PathBuf::from("/fixture"),
                original_filename: "fixture".to_owned(),
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

fn dst(values: &[(i64, Option<f64>)]) -> ParsedFile {
    ParsedFile::Dst(
        values
            .iter()
            .map(|(epoch_ms, value)| crate::parse::HourlyRecord {
                epoch_ms: *epoch_ms,
                value: *value,
            })
            .collect(),
    )
}

#[tokio::test]
async fn monthly_active_history_and_explicit_missing_are_transactional() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(1, 1, IndexEdition::Final),
        dst(&[(0, Some(10.0)), (1, None)]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(2, 1, IndexEdition::Provisional),
        dst(&[(0, Some(99.0)), (1, Some(4.0))]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(3, 1, IndexEdition::Provisional),
        dst(&[(0, Some(88.0)), (1, Some(5.0))]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(4, 2, IndexEdition::Final),
        dst(&[(1, Some(3.0))]),
    )
    .await
    .unwrap();
    let connection = database.database.connect().unwrap();
    let mut rows = connection
        .query("SELECT COUNT(*) FROM active_release", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        2
    );
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM retained_history_release WHERE partition_key = 'month-01'",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        1
    );
    let mut rows = connection.query("SELECT d.dst_nt, d.quality, source.edition, source.artifact_sha256 FROM dst_hourly d JOIN active_release a ON a.release_id = d.release_id JOIN sample_provenance p ON p.release_id = d.release_id AND p.sample_key = 'dst_hourly/' || d.epoch_utc_ms JOIN dataset_release source ON source.release_id = p.source_release_id WHERE a.partition_key = 'month-01' ORDER BY d.epoch_utc_ms", ()).await.unwrap();
    let first = rows.next().await.unwrap().unwrap();
    assert_eq!(first.get::<Option<f64>>(0).unwrap(), Some(10.0));
    assert_eq!(first.get::<String>(1).unwrap(), "final");
    assert_eq!(first.get::<String>(2).unwrap(), "final");
    assert_eq!(
        first.get::<String>(3).unwrap(),
        Sha256Digest::from_bytes([1; 32]).to_string()
    );
    let second = rows.next().await.unwrap().unwrap();
    assert_eq!(second.get::<Option<f64>>(0).unwrap(), Some(5.0));
    assert_eq!(second.get::<String>(1).unwrap(), "provisional");
    assert_eq!(second.get::<String>(2).unwrap(), "provisional");
    assert_eq!(
        second.get::<String>(3).unwrap(),
        Sha256Digest::from_bytes([3; 32]).to_string()
    );
    let mut changes = connection.query("SELECT field_name, old_value_json, new_value_json FROM release_field_change WHERE sample_key = 'dst_hourly/1' AND old_value_json = '4'", ()).await.unwrap();
    let latest = changes.next().await.unwrap().unwrap();
    assert_eq!(latest.get::<String>(0).unwrap(), "value");
    assert_eq!(latest.get::<String>(1).unwrap(), "4");
    assert_eq!(latest.get::<String>(2).unwrap(), "5");
    assert!(
        is_applied(&database, "source-3", Sha256Digest::from_bytes([3; 32]))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn reactivates_a_after_b_without_scanning_history() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file_for_source(10, 1, IndexEdition::Final, "stable-final-source"),
        dst(&[(0, Some(1.0))]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file_for_source(11, 1, IndexEdition::Final, "stable-final-source"),
        dst(&[(0, Some(2.0))]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file_for_source(10, 1, IndexEdition::Final, "stable-final-source"),
        dst(&[(0, Some(1.0))]),
    )
    .await
    .unwrap();
    let connection = database.database.connect().unwrap();
    let mut rows = connection.query("SELECT d.dst_nt FROM dst_hourly d JOIN active_release a ON a.release_id = d.release_id WHERE a.partition_key = 'month-01'", ()).await.unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<Option<f64>>(0)
            .unwrap(),
        Some(1.0)
    );
}

#[tokio::test]
async fn repeated_final_and_provisional_refresh_is_event_idempotent() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    let final_file = file_for_source(50, 1, IndexEdition::Final, "stable-final-source");
    let provisional_file = file_for_source(
        51,
        1,
        IndexEdition::Provisional,
        "stable-provisional-source",
    );
    let final_values = || dst(&[(0, Some(10.0)), (1, None)]);
    let provisional_values = || dst(&[(0, Some(99.0)), (1, Some(4.0))]);
    import_parsed_file(&database, &final_file, final_values())
        .await
        .unwrap();
    import_parsed_file(&database, &provisional_file, provisional_values())
        .await
        .unwrap();
    let before = snapshot_state(&database).await;

    let final_repeat = import_parsed_file(&database, &final_file, final_values())
        .await
        .unwrap();
    let provisional_repeat = import_parsed_file(&database, &provisional_file, provisional_values())
        .await
        .unwrap();

    assert_eq!(final_repeat.releases_reused, 1);
    assert_eq!(final_repeat.changes, 0);
    assert_eq!(provisional_repeat.releases_reused, 1);
    assert_eq!(provisional_repeat.changes, 0);
    assert_eq!(snapshot_state(&database).await, before);
    let connection = database.database.connect().unwrap();
    let mut rows = connection.query("SELECT d.dst_nt FROM dst_hourly d JOIN active_release a ON a.release_id=d.release_id WHERE a.partition_key='month-01' ORDER BY d.epoch_utc_ms", ()).await.unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<Option<f64>>(0)
            .unwrap(),
        Some(10.0)
    );
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<Option<f64>>(0)
            .unwrap(),
        Some(4.0)
    );
}

async fn snapshot_state(database: &crate::database::YearDatabase) -> (String, String, i64) {
    let connection = database.database.connect().unwrap();
    let mut rows = connection.query("SELECT a.release_id, h.release_id, (SELECT COUNT(*) FROM release_change) FROM active_release a JOIN retained_history_release h USING (partition_key) WHERE a.partition_key='month-01'", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    (
        row.get(0).unwrap(),
        row.get(1).unwrap(),
        row.get(2).unwrap(),
    )
}

#[tokio::test]
async fn database_retention_follows_a_b_c_d_e_contract() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    let states = [
        (20, dst(&[(0, Some(1.0))])),
        (21, dst(&[(0, Some(1.0)), (1, Some(2.0))])),
        (22, dst(&[(0, Some(3.0)), (1, Some(2.0))])),
        (23, dst(&[(0, Some(3.0)), (1, Some(2.0)), (2, Some(4.0))])),
        (24, dst(&[(0, Some(5.0)), (1, Some(2.0)), (2, Some(4.0))])),
    ];
    let mut release_ids = Vec::new();
    for (digest, parsed) in states {
        import_parsed_file(&database, &file(digest, 1, IndexEdition::Final), parsed)
            .await
            .unwrap();
        release_ids.push(release_id(&file(digest, 1, IndexEdition::Final)));
    }
    let connection = database.database.connect().unwrap();
    let mut rows = connection.query("SELECT a.release_id, h.release_id FROM active_release a JOIN retained_history_release h USING (partition_key) WHERE a.partition_key = 'month-01'", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), release_ids[4]);
    assert_eq!(row.get::<String>(1).unwrap(), release_ids[3]);
    let mut kinds = connection
        .query(
            "SELECT change_kind FROM release_change ORDER BY change_id",
            (),
        )
        .await
        .unwrap();
    let mut actual = Vec::new();
    while let Some(row) = kinds.next().await.unwrap() {
        actual.push(row.get::<String>(0).unwrap());
    }
    assert_eq!(
        actual,
        ["initial", "append", "revision", "append", "revision"]
    );
}

#[tokio::test]
async fn readonly_candidate_validation_rejects_same_maturity_truncation() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(30, 1, IndexEdition::Final),
        dst(&[(0, Some(1.0)), (1, Some(2.0))]),
    )
    .await
    .unwrap();
    let candidate = dst(&[(0, Some(1.0))]);
    assert!(
        validate_candidate(&database, &file(31, 1, IndexEdition::Final), &candidate)
            .await
            .is_err()
    );
    let connection = database.database.connect().unwrap();
    let mut rows = connection
        .query("SELECT COUNT(*) FROM dataset_release", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        1
    );
}

#[tokio::test]
async fn same_maturity_explicit_null_replaces_an_available_value() {
    let temporary = TempDir::new().unwrap();
    let database = crate::database::open_year(
        temporary.path().join("2020.db"),
        crate::IndexDataset::Dst,
        2020,
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(40, 1, IndexEdition::Final),
        dst(&[(0, Some(1.0))]),
    )
    .await
    .unwrap();
    import_parsed_file(
        &database,
        &file(41, 1, IndexEdition::Final),
        dst(&[(0, None)]),
    )
    .await
    .unwrap();
    let connection = database.database.connect().unwrap();
    let mut rows = connection
        .query("SELECT d.dst_nt FROM dst_hourly d JOIN active_release a ON a.release_id = d.release_id WHERE a.partition_key = 'month-01'", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<Option<f64>>(0)
            .unwrap(),
        None
    );
}
