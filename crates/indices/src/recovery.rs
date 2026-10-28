//! Local-first recovery helpers used by bounded synchronization.

use crate::{
    ChangeSummary, IndexDataset, IndexError,
    maintenance::{SyncPolicy, sync_dataset_interval},
    range::{CoverageGap, CoverageGapReason, SourceAttempt, SyncMode},
};
use ionoray_store::MaintenanceSummary;

pub(crate) struct IntervalRecovery {
    pub(crate) records: usize,
    pub(crate) attempts: Vec<SourceAttempt>,
    pub(crate) summary: MaintenanceSummary,
    pub(crate) changes: ChangeSummary,
}

pub(crate) async fn recover_interval(
    store: &ionoray_store::StoreRoot,
    dataset: IndexDataset,
    start_utc_ms: i64,
    end_utc_ms: i64,
    mode: SyncMode,
    force: bool,
    gaps: &[CoverageGap],
) -> Result<IntervalRecovery, IndexError> {
    let year = u16::try_from(
        crate::time::epoch_from_millis(start_utc_ms)
            .to_gregorian_utc()
            .0,
    )
    .map_err(|_| IndexError::InvalidNumber("range year"))?;
    let policy = match (mode, force) {
        (SyncMode::Offline, _) => SyncPolicy::Offline,
        (_, true) => SyncPolicy::ForceDownload,
        _ => SyncPolicy::AlwaysCheck,
    };
    let ranges = requested_ranges(dataset, start_utc_ms, end_utc_ms, mode, gaps);
    let mut records = 0;
    let mut attempts = Vec::new();
    let mut summary = MaintenanceSummary::default();
    let mut changes = ChangeSummary::default();
    for (start, end) in ranges {
        match sync_dataset_interval(store, dataset, start, end, policy).await {
            Ok(outcome) => {
                records += outcome.report.records_imported;
                add_summary(&mut summary, outcome.report.summary);
                changes.add(outcome.report.changes);
                attempts.extend(outcome.files.iter().map(success_attempt));
                attempts.push(attempt(
                    year,
                    None,
                    policy == SyncPolicy::Offline,
                    None,
                    "synchronized",
                ));
                attempts.extend(outcome.failures.iter().map(|failure| SourceAttempt {
                    year: failure.year,
                    month: failure.month,
                    day: failure.day,
                    local_recovery: false,
                    gap_reason: Some(reason(failure.kind)),
                    source_id: failure.source_id.clone(),
                    canonical_url: failure.canonical_url.clone(),
                    edition: failure.edition,
                    artifact_sha256: None,
                    origin: None,
                    committed: false,
                    outcome: failure.diagnostic(),
                }));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(IntervalRecovery {
        records,
        attempts,
        summary,
        changes,
    })
}

fn add_summary(total: &mut MaintenanceSummary, part: MaintenanceSummary) {
    total.sources_queried += part.sources_queried;
    total.metadata_unchanged += part.metadata_unchanged;
    total.bodies_downloaded += part.bodies_downloaded;
    total.bytes_downloaded += part.bytes_downloaded;
    total.artifacts_created += part.artifacts_created;
    total.releases_created += part.releases_created;
    total.releases_reused += part.releases_reused;
}

fn success_attempt(file: &crate::ValidatedIndexFile) -> SourceAttempt {
    SourceAttempt {
        year: file.year,
        month: file.month,
        day: file.day,
        local_recovery: file.origin == crate::IndexFileOrigin::LocalCas,
        gap_reason: None,
        source_id: Some(file.source_id.clone()),
        canonical_url: file.canonical_url.clone(),
        edition: Some(file.edition),
        artifact_sha256: Some(file.download.artifact.digest.to_string()),
        origin: Some(file.origin),
        committed: true,
        outcome: "accepted and imported".to_owned(),
    }
}

fn requested_ranges(
    dataset: IndexDataset,
    start: i64,
    end: i64,
    mode: SyncMode,
    gaps: &[CoverageGap],
) -> Vec<(i64, i64)> {
    if mode != SyncMode::Ensure
        || matches!(
            dataset,
            IndexDataset::KpApF107 | IndexDataset::IriIgRz | IndexDataset::IriApF107
        )
    {
        return vec![(start, end)];
    }
    let mut ranges = gaps
        .iter()
        .flat_map(|gap| {
            let mut result = Vec::new();
            let mut cursor = gap.start_utc_ms.max(start);
            while cursor < gap.end_utc_ms.min(end) {
                let (begin, finish) = if dataset == IndexDataset::Ae
                    && crate::time::epoch_from_millis(cursor).to_gregorian_utc().0 >= 2021
                {
                    let begin = cursor.div_euclid(86_400_000) * 86_400_000;
                    (begin, begin + 86_400_000)
                } else {
                    month_range(cursor)
                };
                result.push((begin.max(start), finish.min(end)));
                cursor = finish;
            }
            result
        })
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

fn month_range(epoch_ms: i64) -> (i64, i64) {
    let (year, month, ..) = crate::time::epoch_from_millis(epoch_ms).to_gregorian_utc();
    let start = ionoray_core::Epoch::maybe_from_gregorian_utc(year, month, 1, 0, 0, 0, 0)
        .expect("Gregorian date from epoch is valid");
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let end = ionoray_core::Epoch::maybe_from_gregorian_utc(next_year, next_month, 1, 0, 0, 0, 0)
        .expect("next Gregorian month is valid");
    (
        crate::time::epoch_millis(start).expect("finite month start"),
        crate::time::epoch_millis(end).expect("finite month end"),
    )
}

fn attempt(
    year: u16,
    month: Option<u8>,
    local_recovery: bool,
    gap_reason: Option<CoverageGapReason>,
    outcome: &str,
) -> SourceAttempt {
    SourceAttempt {
        year,
        month,
        day: None,
        local_recovery,
        gap_reason,
        source_id: None,
        canonical_url: None,
        edition: None,
        artifact_sha256: None,
        origin: None,
        committed: false,
        outcome: outcome.to_owned(),
    }
}

fn reason(kind: crate::download::IntervalFailureKind) -> CoverageGapReason {
    match kind {
        crate::download::IntervalFailureKind::RemoteUnavailable => {
            CoverageGapReason::RemoteUnavailable
        }
        crate::download::IntervalFailureKind::Network => CoverageGapReason::NetworkFailure,
        crate::download::IntervalFailureKind::Parse => CoverageGapReason::ParseFailure,
        crate::download::IntervalFailureKind::Unsupported => CoverageGapReason::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ae_ensure_visits_each_missing_day_within_requested_bounds() {
        let start = crate::parse::epoch_ms(2024, 5, 1, 12).unwrap();
        let end = crate::parse::epoch_ms(2024, 5, 3, 1).unwrap();
        let gap = CoverageGap {
            field: crate::IndexField::Ae,
            start_utc_ms: start,
            end_utc_ms: end,
            reason: CoverageGapReason::LocalMissing,
        };
        let ranges = requested_ranges(IndexDataset::Ae, start, end, SyncMode::Ensure, &[gap]);
        assert_eq!(
            ranges,
            vec![
                (start, crate::parse::epoch_ms(2024, 5, 2, 0).unwrap()),
                (
                    crate::parse::epoch_ms(2024, 5, 2, 0).unwrap(),
                    crate::parse::epoch_ms(2024, 5, 3, 0).unwrap()
                ),
                (crate::parse::epoch_ms(2024, 5, 3, 0).unwrap(), end),
            ]
        );
    }
}
