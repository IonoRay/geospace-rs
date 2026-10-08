use tempfile::TempDir;

use super::*;
use crate::database::open_year;

#[tokio::test]
async fn geomagnetic_query_does_not_require_daily_solar_flux() {
    let temporary = TempDir::new().unwrap();
    let year = 2020;
    let database = open_year(
        temporary.path().join("gfz.db"),
        IndexDataset::KpApF107,
        year,
    )
    .await
    .unwrap();
    let epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap();
    let epoch_ms = epoch_millis(epoch).unwrap();
    let connection = database.database.connect().unwrap();
    connection
            .execute(
                "INSERT INTO dataset_release (release_id, artifact_sha256, source_id, edition, edition_priority, parser_name, parser_version, coverage_start_utc_ms, coverage_end_utc_ms, fetched_at_utc_ms, imported_at_utc_ms, record_count, state) VALUES ('release', ?, 'test', 'rolling', 0, 'test', '1', ?, ?, ?, ?, 1, 'ready')",
                turso::params![
                    Sha256Digest::from_bytes([1; 32]).to_string(),
                    epoch_ms,
                    epoch_ms + DAY_MS,
                    epoch_ms,
                    epoch_ms,
                ],
            )
            .await
            .unwrap();
    connection
            .execute(
                "INSERT INTO geomagnetic_3h (release_id, interval_start_utc_ms, kp_thirds, ap) VALUES ('release', ?, 9, 15.0)",
                [epoch_ms],
            )
            .await
            .unwrap();
    connection
        .execute(
            "INSERT INTO active_release (partition_key, release_id) VALUES ('rolling', 'release')",
            (),
        )
        .await
        .unwrap();
    connection
            .execute(
                "INSERT INTO sample_provenance (release_id, sample_key, source_release_id) VALUES ('release', ?, 'release')",
                [format!("geomagnetic_3h/{epoch_ms}")],
            )
            .await
            .unwrap();

    let values = read_geomagnetic_indices(&database.database, epoch)
        .await
        .unwrap();
    assert!((values.kp.value.value() - 3.0).abs() < f64::EPSILON);
    assert!((values.ap.value.value() - 15.0).abs() < f64::EPSILON);
    assert!(matches!(
        read_daily_ap(&database.database, epoch).await,
        Err(IndexError::MissingValue { .. })
    ));
    connection
        .execute(
            "UPDATE geomagnetic_3h SET kp_thirds = NULL WHERE release_id = 'release'",
            (),
        )
        .await
        .unwrap();
    let ap_only = read_ap(&database.database, epoch).await.unwrap();
    assert!((ap_only.value.value() - 15.0).abs() < f64::EPSILON);
    assert_eq!(ap_only, values.ap);
    assert!(matches!(
        read_geomagnetic_indices(&database.database, epoch).await,
        Err(IndexError::MissingValue { .. })
    ));
}
