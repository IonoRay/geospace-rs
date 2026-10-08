//! Live 2020 synchronization kept out of the default offline test suite.

use crate::{IndexDataset, IndexField, IndexStore, RangeRequest, SyncMode, SyncPolicy};
use ionoray_core::Epoch;
use tempfile::TempDir;

#[tokio::test]
#[ignore = "downloads GFZ and WDC Kyoto source files"]
async fn synchronizes_verifies_and_reads_2020() {
    let temporary = TempDir::new().unwrap();
    let store = IndexStore::open(Some(temporary.path())).await.unwrap();
    let sync = store
        .sync_year(2020, SyncPolicy::AlwaysCheck)
        .await
        .unwrap();
    assert_eq!(sync.summary.sources_queried, 25);

    let verified = store.verify_year(2020).await.unwrap();
    assert_eq!(
        verified
            .datasets
            .iter()
            .map(|dataset| dataset.ready_partitions)
            .collect::<Vec<_>>(),
        [1, 12, 12, 1, 1]
    );

    let values = store
        .at("2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap())
        .await
        .unwrap();
    assert!(values.kp.value.value() >= 0.0);
}

#[tokio::test]
#[ignore = "downloads the official rolling IRI index files"]
async fn synchronizes_and_reads_iri_2020() {
    let temporary = TempDir::new().unwrap();
    let store = IndexStore::open(Some(temporary.path())).await.unwrap();
    let epoch = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
    for (dataset, fields) in [
        (
            IndexDataset::IriIgRz,
            vec![IndexField::Ig12, IndexField::Rz12],
        ),
        (
            IndexDataset::IriApF107,
            vec![IndexField::IriF107, IndexField::IriF107a81],
        ),
    ] {
        let report = store
            .sync_range(RangeRequest {
                dataset,
                fields,
                start: epoch,
                end: epoch + ionoray_core::Duration::from_milliseconds(1.0),
                mode: SyncMode::Refresh,
                force: false,
            })
            .await
            .unwrap();
        assert_eq!(report.download_summary.sources_queried, 1);
    }

    let values = store.iri_at(epoch).await.unwrap();
    assert!(values.monthly.rz12.value.value() >= 0.0);
    assert!(values.f107.daily.value.value() > 0.0);
}
