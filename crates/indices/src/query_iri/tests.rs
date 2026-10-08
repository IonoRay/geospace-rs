use super::*;
use crate::database::open_year;

// Synthetic one-day records exercise field isolation; no upstream truth is implied.
async fn fixture(dataset: IndexDataset) -> (tempfile::TempDir, Database, Epoch) {
    let home = tempfile::tempdir().unwrap();
    let database = open_year(home.path().join("fixture.db"), dataset, 2020)
        .await
        .unwrap()
        .database;
    let epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap();
    let start = epoch_millis(epoch).unwrap() - DAY_MS / 2;
    let connection = database.connect().unwrap();
    connection.execute(
        "INSERT INTO dataset_release (release_id, artifact_sha256, source_id, edition, edition_priority, parser_name, parser_version, coverage_start_utc_ms, coverage_end_utc_ms, fetched_at_utc_ms, imported_at_utc_ms, record_count, state) VALUES ('fixture', ?, 'synthetic', 'rolling', 0, 'fixture', '1', ?, ?, ?, ?, 1, 'ready')",
        turso::params![Sha256Digest::from_bytes([1; 32]).to_string(), start, start + DAY_MS, start, start],
    ).await.unwrap();
    connection
        .execute(
            "INSERT INTO active_release VALUES ('rolling', 'fixture')",
            (),
        )
        .await
        .unwrap();
    let table = if dataset == IndexDataset::IriIgRz {
        "iri_ig_rz_daily"
    } else {
        "iri_f107_daily"
    };
    connection
        .execute(
            "INSERT INTO sample_provenance VALUES ('fixture', ?, 'fixture')",
            [format!("{table}/{start}")],
        )
        .await
        .unwrap();
    let sql = if dataset == IndexDataset::IriIgRz {
        "INSERT INTO iri_ig_rz_daily VALUES ('fixture', ?, -5.5, 5.0, 'final')"
    } else {
        "INSERT INTO iri_f107_daily VALUES ('fixture', ?, 71.2, 72.1, 73.7)"
    };
    connection.execute(sql, [start]).await.unwrap();
    (home, database, epoch)
}

#[tokio::test]
async fn monthly_fields_preserve_provenance_and_isolate_validation() {
    let (_home, database, epoch) = fixture(IndexDataset::IriIgRz).await;
    let combined = read_monthly(&database, epoch).await.unwrap();
    assert_eq!(read_ig12(&database, epoch).await.unwrap(), combined.ig12);
    assert_eq!(read_rz12(&database, epoch).await.unwrap(), combined.rz12);
    let connection = database.connect().unwrap();
    connection
        .execute("UPDATE iri_ig_rz_daily SET rz12 = 1e999", ())
        .await
        .unwrap();
    assert_eq!(read_ig12(&database, epoch).await.unwrap(), combined.ig12);
    assert!(read_rz12(&database, epoch).await.is_err());
    // The old aggregate read fails, proving the test distinguishes both paths.
    assert!(read_monthly(&database, epoch).await.is_err());
}

#[tokio::test]
async fn flux_fields_do_not_require_unused_averages() {
    let (_home, database, epoch) = fixture(IndexDataset::IriApF107).await;
    let combined = read_f107(&database, epoch).await.unwrap();
    assert_eq!(
        read_f107_daily(&database, epoch).await.unwrap(),
        combined.daily
    );
    assert_eq!(
        read_f107_81_day(&database, epoch).await.unwrap(),
        combined.average_81_day
    );
    let connection = database.connect().unwrap();
    connection
        .execute(
            "UPDATE iri_f107_daily SET f107a_365d_adjusted_sfu = 1e999",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        read_f107_daily(&database, epoch).await.unwrap(),
        combined.daily
    );
    assert_eq!(
        read_f107_81_day(&database, epoch).await.unwrap(),
        combined.average_81_day
    );
    assert!(read_f107(&database, epoch).await.is_err());
    connection
        .execute(
            "UPDATE iri_f107_daily SET f107a_81d_adjusted_sfu = 1e999",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        read_f107_daily(&database, epoch).await.unwrap(),
        combined.daily
    );
    assert!(read_f107_81_day(&database, epoch).await.is_err());
    connection
        .execute(
            "UPDATE iri_f107_daily SET f107a_81d_adjusted_sfu = 72.1, f107_adjusted_sfu = 1e999",
            (),
        )
        .await
        .unwrap();
    assert_eq!(
        read_f107_81_day(&database, epoch).await.unwrap(),
        combined.average_81_day
    );
    assert!(read_f107_daily(&database, epoch).await.is_err());
    assert!(matches!(
        read_f107_daily(&database, epoch + ionoray_core::Duration::from_days(1.0)).await,
        Err(IndexError::MissingValue { .. })
    ));
}
