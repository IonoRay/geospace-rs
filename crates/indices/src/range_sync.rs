//! Bounded range synchronization orchestration.

use ionoray_store::StoreRoot;

use crate::{
    IndexError,
    coverage::audit,
    range::{
        CoverageGapReason, RangeRequest, RangeSyncReport, RangeSyncStatus, SourceCheckStatus,
        SyncMode,
    },
    recovery::recover_interval,
};

/// Synchronizes one requested dataset interval and reports every residual gap.
///
/// The caller receives a successful partial report when individual partitions
/// cannot be recovered. Invalid requests remain errors because no meaningful
/// maintenance operation can be planned for them.
pub(crate) async fn sync_range(
    store: &StoreRoot,
    request: RangeRequest,
) -> Result<RangeSyncReport, IndexError> {
    let (start, end) = request.bounds_ms()?;
    let local = recover_interval(
        store,
        request.dataset,
        start,
        end,
        SyncMode::Offline,
        false,
        &[],
    )
    .await?;
    let mut records_imported = local.records;
    let mut attempts = local.attempts;
    let mut download_summary = local.summary;
    let mut changes = local.changes;
    let before = audit(
        store,
        request.dataset,
        &request.fields,
        start,
        end,
        CoverageGapReason::LocalMissing,
    )
    .await?;
    let local_complete = before.iter().all(|field| field.gaps.is_empty());
    let missing = before
        .iter()
        .flat_map(|field| field.gaps.clone())
        .collect::<Vec<_>>();
    let check_sources = request.mode != SyncMode::Offline
        && !(request.mode == SyncMode::Ensure && local_complete && !request.force);
    if check_sources {
        let online = recover_interval(
            store,
            request.dataset,
            start,
            end,
            request.mode,
            request.force,
            &missing,
        )
        .await?;
        records_imported += online.records;
        attempts.extend(online.attempts);
        add_summary(&mut download_summary, online.summary);
        changes.add(online.changes);
    }
    let coverage = final_coverage(store, &request, start, end, &attempts).await?;
    let gaps = coverage
        .iter()
        .flat_map(|field| field.gaps.clone())
        .collect::<Vec<_>>();
    let (coverage_status, source_check_status, status) = classify(&gaps, &attempts, check_sources);
    Ok(RangeSyncReport {
        dataset: request.dataset,
        mode: request.mode,
        force: request.force,
        requested_start_utc_ms: start,
        requested_end_utc_ms: end,
        coverage,
        gaps,
        attempts,
        coverage_status,
        source_check_status,
        status,
        records_imported,
        download_summary,
        changes,
    })
}

fn classify(
    gaps: &[crate::CoverageGap],
    attempts: &[crate::SourceAttempt],
    check_sources: bool,
) -> (RangeSyncStatus, SourceCheckStatus, RangeSyncStatus) {
    let coverage_status = if gaps.is_empty() {
        RangeSyncStatus::Complete
    } else {
        RangeSyncStatus::Partial
    };
    let source_check_status = if !check_sources {
        SourceCheckStatus::NotRequested
    } else if attempts
        .iter()
        .any(|attempt| !attempt.local_recovery && attempt.gap_reason.is_some())
    {
        SourceCheckStatus::Failed
    } else {
        SourceCheckStatus::Complete
    };
    let status = if coverage_status == RangeSyncStatus::Complete
        && source_check_status != SourceCheckStatus::Failed
    {
        RangeSyncStatus::Complete
    } else {
        RangeSyncStatus::Partial
    };
    (coverage_status, source_check_status, status)
}

fn add_summary(
    total: &mut ionoray_store::MaintenanceSummary,
    part: ionoray_store::MaintenanceSummary,
) {
    total.sources_queried += part.sources_queried;
    total.metadata_unchanged += part.metadata_unchanged;
    total.bodies_downloaded += part.bodies_downloaded;
    total.bytes_downloaded += part.bytes_downloaded;
    total.artifacts_created += part.artifacts_created;
    total.releases_created += part.releases_created;
    total.releases_reused += part.releases_reused;
}

async fn final_coverage(
    store: &StoreRoot,
    request: &RangeRequest,
    start: i64,
    end: i64,
    attempts: &[crate::SourceAttempt],
) -> Result<Vec<crate::FieldCoverage>, IndexError> {
    let absent_reason = match request.mode {
        SyncMode::Offline => CoverageGapReason::OfflineUnverified,
        SyncMode::Ensure | SyncMode::Refresh => CoverageGapReason::UpstreamMissing,
    };
    let mut coverage = audit(
        store,
        request.dataset,
        &request.fields,
        start,
        end,
        absent_reason,
    )
    .await?;
    for field in &mut coverage {
        field.gaps = field
            .gaps
            .iter()
            .flat_map(|gap| split_at_source_boundaries(gap, request.dataset))
            .collect();
        for gap in &mut field.gaps {
            annotate_gap(gap, attempts);
        }
    }
    Ok(coverage)
}

fn annotate_gap(gap: &mut crate::CoverageGap, attempts: &[crate::SourceAttempt]) {
    let same_partition = |attempt: &crate::SourceAttempt| {
        attempt.month.is_some_and(|month| {
            same_month(gap.start_utc_ms, attempt.year, month)
                && attempt.day.is_none_or(|day| {
                    crate::time::epoch_from_millis(gap.start_utc_ms)
                        .to_gregorian_utc()
                        .2
                        == day
                })
        })
    };
    if attempts
        .iter()
        .any(|attempt| attempt.committed && same_partition(attempt))
    {
        return;
    }
    if let Some(reason) = attempts
        .iter()
        .find(|attempt| same_partition(attempt))
        .and_then(|attempt| attempt.gap_reason)
    {
        gap.reason = reason;
    }
}

fn split_at_source_boundaries(
    gap: &crate::CoverageGap,
    dataset: crate::IndexDataset,
) -> Vec<crate::CoverageGap> {
    split_at_month_boundaries(gap)
        .into_iter()
        .flat_map(|gap| {
            if dataset != crate::IndexDataset::Ae
                || crate::time::epoch_from_millis(gap.start_utc_ms)
                    .to_gregorian_utc()
                    .0
                    < 2021
            {
                return vec![gap];
            }
            let mut result = Vec::new();
            let mut start = gap.start_utc_ms;
            while start < gap.end_utc_ms {
                let end = ((start.div_euclid(86_400_000) + 1) * 86_400_000).min(gap.end_utc_ms);
                result.push(crate::CoverageGap {
                    start_utc_ms: start,
                    end_utc_ms: end,
                    ..gap.clone()
                });
                start = end;
            }
            result
        })
        .collect()
}

fn split_at_month_boundaries(gap: &crate::CoverageGap) -> Vec<crate::CoverageGap> {
    let mut result = Vec::new();
    let mut start = gap.start_utc_ms;
    while start < gap.end_utc_ms {
        let end = next_month_start(start).min(gap.end_utc_ms);
        result.push(crate::CoverageGap {
            field: gap.field,
            start_utc_ms: start,
            end_utc_ms: end,
            reason: gap.reason,
        });
        start = end;
    }
    result
}

fn next_month_start(epoch_ms: i64) -> i64 {
    let (year, month, ..) = crate::time::epoch_from_millis(epoch_ms).to_gregorian_utc();
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    crate::time::epoch_millis(
        ionoray_core::Epoch::maybe_from_gregorian_utc(next_year, next_month, 1, 0, 0, 0, 0)
            .expect("valid Gregorian month"),
    )
    .expect("finite month boundary")
}

fn same_month(epoch_ms: i64, year: u16, month: u8) -> bool {
    let date = crate::time::epoch_from_millis(epoch_ms).to_gregorian_utc();
    date.0 == i32::from(year) && date.1 == month
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_refresh() -> crate::SourceAttempt {
        crate::SourceAttempt {
            year: 2020,
            month: None,
            day: None,
            local_recovery: false,
            gap_reason: Some(CoverageGapReason::NetworkFailure),
            source_id: Some("synthetic.source".to_owned()),
            canonical_url: None,
            edition: None,
            artifact_sha256: None,
            origin: None,
            committed: false,
            outcome: "synthetic connection refused".to_owned(),
        }
    }

    #[test]
    fn full_local_coverage_does_not_hide_failed_refresh() {
        assert_eq!(
            classify(&[], &[failed_refresh()], true),
            (
                RangeSyncStatus::Complete,
                SourceCheckStatus::Failed,
                RangeSyncStatus::Partial,
            )
        );
        assert_eq!(
            classify(&[], &[], false),
            (
                RangeSyncStatus::Complete,
                SourceCheckStatus::NotRequested,
                RangeSyncStatus::Complete,
            )
        );
    }

    #[test]
    fn gap_and_successful_source_check_have_distinct_statuses() {
        let gap = crate::CoverageGap {
            field: crate::IndexField::Dst,
            start_utc_ms: 0,
            end_utc_ms: 1,
            reason: CoverageGapReason::UpstreamMissing,
        };
        assert_eq!(
            classify(&[gap], &[], true),
            (
                RangeSyncStatus::Partial,
                SourceCheckStatus::Complete,
                RangeSyncStatus::Partial,
            )
        );
    }

    #[test]
    fn splits_one_gap_at_utc_month_boundary() {
        let start = crate::time::epoch_millis(
            ionoray_core::Epoch::maybe_from_gregorian_utc(2020, 1, 31, 23, 0, 0, 0).unwrap(),
        )
        .unwrap();
        let end = crate::time::epoch_millis(
            ionoray_core::Epoch::maybe_from_gregorian_utc(2020, 2, 1, 1, 0, 0, 0).unwrap(),
        )
        .unwrap();
        let parts = split_at_month_boundaries(&crate::CoverageGap {
            field: crate::IndexField::Dst,
            start_utc_ms: start,
            end_utc_ms: end,
            reason: CoverageGapReason::NetworkFailure,
        });
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].end_utc_ms, parts[1].start_utc_ms);
    }
    #[test]
    fn ae_gaps_cross_from_monthly_to_daily_and_match_exact_attempt_day() {
        let start = crate::parse::epoch_ms(2020, 12, 31, 23).unwrap();
        let end = crate::parse::epoch_ms(2021, 1, 3, 0).unwrap();
        let gap = crate::CoverageGap {
            field: crate::IndexField::Ae,
            start_utc_ms: start,
            end_utc_ms: end,
            reason: CoverageGapReason::UpstreamMissing,
        };
        let parts = split_at_source_boundaries(&gap, crate::IndexDataset::Ae);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[1].end_utc_ms,
            crate::parse::epoch_ms(2021, 1, 2, 0).unwrap()
        );
        let mut failed = failed_refresh();
        failed.year = 2021;
        failed.month = Some(1);
        failed.day = Some(1);
        let mut successful = failed.clone();
        successful.day = Some(2);
        successful.committed = true;
        successful.gap_reason = None;
        let mut first = parts[1].clone();
        annotate_gap(&mut first, &[successful, failed]);
        assert_eq!(first.reason, CoverageGapReason::NetworkFailure);
    }
}
