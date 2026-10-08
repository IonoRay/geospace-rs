use tempfile::TempDir;

use super::*;
use crate::{
    CoverageGapReason, IndexField, RangeRequest, RangeSyncStatus, SourceCheckStatus, SyncMode,
};

#[tokio::test]
async fn explicit_init_and_open_share_the_same_idempotent_path() {
    let temporary = TempDir::new().unwrap();
    let report = IndexStore::init(Some(temporary.path())).await.unwrap();
    assert_eq!(report.catalogs.len(), 5);
    assert!(report.catalogs.iter().all(|catalog| catalog.is_file()));
    IndexStore::open(Some(temporary.path())).await.unwrap();
}

#[tokio::test]
async fn offline_range_prepares_and_typed_reads_keep_provenance() {
    let temporary = TempDir::new().unwrap();
    let indices = IndexStore::open(Some(temporary.path())).await.unwrap();
    let epoch = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();

    for (dataset, fields) in [
        (
            IndexDataset::KpApF107,
            vec![IndexField::Ap3h, IndexField::F107],
        ),
        (
            IndexDataset::IriIgRz,
            vec![IndexField::Ig12, IndexField::Rz12],
        ),
        (
            IndexDataset::IriApF107,
            vec![IndexField::IriF107, IndexField::IriF107a81],
        ),
    ] {
        let report = indices
            .sync_range(request(dataset, fields, epoch, SyncMode::Offline))
            .await
            .unwrap();
        let available = crate::download::cache::for_dataset(dataset, 2020).is_some();
        assert_eq!(
            report.status,
            if available {
                RangeSyncStatus::Complete
            } else {
                RangeSyncStatus::Partial
            }
        );
        assert_eq!(report.download_summary.sources_queried, 0);
        assert_eq!(report.download_summary.bodies_downloaded, 0);
    }

    if crate::download::cache::for_dataset(IndexDataset::KpApF107, 2020).is_none() {
        assert!(indices.ap_at(epoch).await.is_err());
        return;
    }
    let ap = indices.ap_at(epoch).await.unwrap();
    let f107 = indices.f107_at(epoch).await.unwrap();
    let monthly = indices.iri_monthly_at(epoch).await.unwrap();
    let iri_f107 = indices.iri_f107_at(epoch).await.unwrap();
    assert!(ap.value.value() >= 0.0);
    assert!(f107.value.value() > 0.0);
    assert!(monthly.ig12.value.value().is_finite());
    assert!(iri_f107.daily.value.value() > 0.0);
    assert_ne!(ap.release_id.len(), 0);
    assert_eq!(ap.artifact, f107.artifact);
}

#[tokio::test]
async fn offline_2021_f107_gaps_remain_visible_after_import() {
    let temporary = TempDir::new().unwrap();
    let indices = IndexStore::open(Some(temporary.path())).await.unwrap();
    let start = "2021-06-16T12:00:00 UTC".parse::<Epoch>().unwrap();
    indices
        .sync_range(request(
            IndexDataset::KpApF107,
            vec![IndexField::F107, IndexField::F107a],
            start,
            SyncMode::Offline,
        ))
        .await
        .unwrap();

    let missing_day = "2021-06-16T12:00:00 UTC".parse::<Epoch>().unwrap();
    if crate::download::cache::for_dataset(IndexDataset::KpApF107, 2021).is_none() {
        assert!(indices.f107_at(missing_day).await.is_err());
        return;
    }
    let daily = indices.f107_at(missing_day).await.unwrap();
    assert!((daily.value.value() - 80.25).abs() < 1.0e-10);
    assert_eq!(
        daily.derivation,
        crate::ValueDerivation::LinearInterpolation { gap_days: 1 }
    );

    let launch = "2021-06-17T01:22:31.693 UTC".parse::<Epoch>().unwrap();
    let average = indices.f107a_at(launch).await.unwrap();
    assert!((average.value.value() - 79.374_691_358_024_69).abs() < 1.0e-10);
    assert_eq!(
        average.derivation,
        crate::ValueDerivation::CenteredMean {
            window_days: 81,
            interpolated_input_count: 2,
        }
    );
}

#[tokio::test]
async fn range_ensure_reuses_existing_field_coverage() {
    let temporary = TempDir::new().unwrap();
    let indices = IndexStore::open(Some(temporary.path())).await.unwrap();
    let start = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
    let report = indices
        .sync_range(request(
            IndexDataset::KpApF107,
            vec![IndexField::Kp],
            start,
            SyncMode::Offline,
        ))
        .await
        .unwrap();
    if crate::download::cache::for_dataset(IndexDataset::KpApF107, 2020).is_none() {
        assert_eq!(report.status, RangeSyncStatus::Partial);
        assert_eq!(report.download_summary.sources_queried, 0);
        return;
    }
    assert_eq!(report.status, RangeSyncStatus::Complete);
    let report = indices
        .sync_range(request(
            IndexDataset::KpApF107,
            vec![IndexField::Kp],
            start,
            SyncMode::Ensure,
        ))
        .await
        .unwrap();
    assert_eq!(report.status, RangeSyncStatus::Complete);
    assert_eq!(report.coverage_status, RangeSyncStatus::Complete);
    assert_eq!(report.source_check_status, SourceCheckStatus::NotRequested);
    assert!(report.attempts[0].local_recovery);
}

#[tokio::test]
async fn offline_range_reports_missing_data_without_an_http_attempt() {
    let temporary = TempDir::new().unwrap();
    let indices = IndexStore::open(Some(temporary.path())).await.unwrap();
    let epoch = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
    let report = indices
        .sync_range(request(
            IndexDataset::Dst,
            vec![IndexField::Dst],
            epoch,
            SyncMode::Offline,
        ))
        .await
        .unwrap();
    assert_eq!(report.status, RangeSyncStatus::Partial);
    assert_eq!(report.coverage_status, RangeSyncStatus::Partial);
    assert_eq!(report.source_check_status, SourceCheckStatus::NotRequested);
    assert_eq!(report.download_summary.sources_queried, 0);
    assert!(
        report
            .gaps
            .iter()
            .all(|gap| gap.reason == CoverageGapReason::OfflineUnverified)
    );
}

fn request(
    dataset: IndexDataset,
    fields: Vec<IndexField>,
    start: Epoch,
    mode: SyncMode,
) -> RangeRequest {
    RangeRequest {
        dataset,
        fields,
        start,
        end: start + ionoray_core::Duration::from_milliseconds(1.0),
        mode,
        force: false,
    }
}
