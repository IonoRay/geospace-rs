use ionoray_store::{DownloadDisposition, MaintenanceSummary};
use std::collections::HashSet;

use crate::{IndexFileOrigin, YearDownloadReport};

pub(crate) fn summarize_downloads(report: &YearDownloadReport) -> MaintenanceSummary {
    let mut summary = MaintenanceSummary::default();
    let mut observations = HashSet::new();
    for file in &report.files {
        if !observations.insert(&file.download.observation_id) {
            continue;
        }
        summary.sources_queried += 1;
        if file.origin == IndexFileOrigin::BundledCache {
            if file.download.disposition == DownloadDisposition::Downloaded {
                summary.artifacts_created += 1;
            }
            continue;
        }
        match file.download.disposition {
            DownloadDisposition::Downloaded => {
                summary.bodies_downloaded += 1;
                summary.artifacts_created += 1;
                summary.bytes_downloaded += file.download.artifact.byte_size;
            }
            DownloadDisposition::Reused => {
                summary.bodies_downloaded += 1;
                summary.bytes_downloaded += file.download.artifact.byte_size;
            }
            DownloadDisposition::NotModified | DownloadDisposition::MetadataUnchanged => {
                summary.metadata_unchanged += 1;
            }
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ionoray_core::Sha256Digest;
    use ionoray_store::{ArtifactRef, DownloadOutcome};

    use super::*;
    use crate::{IndexDataset, IndexEdition, ValidatedIndexFile};

    #[test]
    fn rolling_expansion_counts_one_physical_observation() {
        let first = file(2020, "rolling-observation");
        let second = file(2021, "rolling-observation");
        let summary = summarize_downloads(&YearDownloadReport {
            year: 2020,
            files: vec![first, second],
        });
        assert_eq!(summary.sources_queried, 1);
        assert_eq!(summary.bodies_downloaded, 1);
        assert_eq!(summary.bytes_downloaded, 17);
    }

    fn file(year: u16, observation_id: &str) -> ValidatedIndexFile {
        ValidatedIndexFile {
            source_id: "gfz.kp-ap-f107.rolling".to_owned(),
            canonical_url: Some("https://example.invalid/rolling".to_owned()),
            dataset: IndexDataset::KpApF107,
            year,
            month: None,
            edition: IndexEdition::Rolling,
            records: 1,
            origin: IndexFileOrigin::Upstream,
            download: DownloadOutcome {
                observation_id: observation_id.to_owned(),
                disposition: DownloadDisposition::Downloaded,
                artifact: ArtifactRef {
                    digest: Sha256Digest::from_bytes([7; 32]),
                    byte_size: 17,
                    path: PathBuf::from("/tmp/test"),
                    original_filename: "rolling.txt".to_owned(),
                    media_type: None,
                    source_modified_at_utc_ms: None,
                    downloaded_at_utc_ms: 0,
                    local_mtime_ns: 0,
                },
                queried_at_utc_ms: 0,
                finished_at_utc_ms: 0,
            },
        }
    }
}
