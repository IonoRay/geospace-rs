use ionoray_core::Sha256Digest;
use ionoray_store::{ArtifactRef, DownloadDisposition, DownloadOutcome};
use tempfile::TempDir;

use super::*;
use crate::{IndexFileOrigin, download::cache};

#[tokio::test]
async fn remote_failure_falls_back_to_valid_bundled_source() {
    let temporary = TempDir::new().unwrap();
    let store = ionoray_store::Store::open_root(Some(temporary.path())).unwrap();
    let source = rolling_source("test.gfz.fallback.2020");
    let files = download_file(&store, source, CheckMode::Metadata)
        .await
        .unwrap()
        .files;
    let file = files.into_iter().next().unwrap();
    assert_eq!(file.file.origin, IndexFileOrigin::BundledCache);
    assert_eq!(file.file.records, 3_294);
}

#[tokio::test]
async fn failed_month_does_not_stop_later_interval_source() {
    let temporary = TempDir::new().unwrap();
    let store = ionoray_store::Store::open_root(Some(temporary.path())).unwrap();
    let failed = SourceFile {
        dataset: crate::IndexDataset::Dst,
        year: 2020,
        month: Some(1),
        logical_name: "dst2001.for.request".to_owned(),
        candidates: vec![SourceCandidate {
            source_id: "test.dst.failed.202001".to_owned(),
            edition: crate::IndexEdition::Final,
            url: "http://127.0.0.1:9/unavailable".to_owned(),
        }],
        cache: None,
    };
    let report = download_interval_sources(
        &store,
        vec![failed, rolling_source("test.gfz.second")],
        CheckMode::Metadata,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(report.failures.len(), 2);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.month == Some(1))
    );
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.month.is_none())
    );
    assert_eq!(report.files.len(), 1);
}

fn rolling_source(source_id: &str) -> SourceFile {
    SourceFile {
        dataset: crate::IndexDataset::KpApF107,
        year: 2020,
        month: None,
        logical_name: "Kp_ap_Ap_SN_F107_since_1932.txt".to_owned(),
        candidates: vec![SourceCandidate {
            source_id: source_id.to_owned(),
            edition: crate::IndexEdition::Rolling,
            url: "http://127.0.0.1:9/unavailable".to_owned(),
        }],
        cache: cache::for_dataset(crate::IndexDataset::KpApF107, 2020),
    }
}

#[test]
fn complete_month_skips_lower_candidate_only_with_all_real_slots() {
    assert!(complete_month(&monthly_file(false)));
    assert!(!complete_month(&monthly_file(true)));
}

fn monthly_file(null_slot: bool) -> PreparedIndexFile {
    let records = (0..744)
        .map(|hour| crate::parse::HourlyRecord {
            epoch_ms: i64::from(hour),
            value: if null_slot && hour == 5 {
                None
            } else {
                Some(1.0)
            },
        })
        .collect::<Vec<_>>();
    PreparedIndexFile {
        file: ValidatedIndexFile {
            source_id: "test.dst.final".to_owned(),
            canonical_url: None,
            dataset: crate::IndexDataset::Dst,
            year: 2020,
            month: Some(1),
            edition: crate::IndexEdition::Final,
            records: records.len(),
            origin: IndexFileOrigin::Upstream,
            download: DownloadOutcome {
                observation_id: "test".to_owned(),
                disposition: DownloadDisposition::Downloaded,
                artifact: ArtifactRef {
                    digest: Sha256Digest::from_bytes([0; 32]),
                    byte_size: 0,
                    path: std::path::PathBuf::new(),
                    original_filename: "dst2001".to_owned(),
                    media_type: None,
                    source_modified_at_utc_ms: None,
                    downloaded_at_utc_ms: 0,
                    local_mtime_ns: 0,
                },
                queried_at_utc_ms: 0,
                finished_at_utc_ms: 0,
            },
        },
        parsed: crate::parse::ParsedFile::Dst(records),
    }
}
