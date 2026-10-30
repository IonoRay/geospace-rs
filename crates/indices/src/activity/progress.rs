use super::{ActivityDataset, ActivityReport};

pub(super) fn start(dataset: ActivityDataset, total_dates: usize) {
    tracing::info!(
        dataset = dataset.name(),
        total_dates,
        phase = "check-accept",
        "activity processing started"
    );
}

pub(super) fn date_start(
    dataset: ActivityDataset,
    date: (u16, u8, u8),
    completed: usize,
    total: usize,
) {
    tracing::info!(
        dataset = dataset.name(),
        year = date.0,
        month = date.1,
        day = date.2,
        completed_dates = completed,
        total_dates = total,
        remaining_dates = total - completed,
        "activity date processing"
    );
}

pub(super) fn date_end(
    dataset: ActivityDataset,
    date: (u16, u8, u8),
    counts: (usize, usize),
    sources: (usize, usize),
    elapsed_secs: u64,
) {
    tracing::info!(
        dataset = dataset.name(),
        year = date.0,
        month = date.1,
        day = date.2,
        completed_dates = counts.0,
        total_dates = counts.1,
        remaining_dates = counts.1 - counts.0,
        checked_sources = sources.0,
        unavailable_candidates = sources.1,
        elapsed_secs,
        "activity date progress"
    );
}

pub(super) fn finish(report: &ActivityReport, elapsed_secs: u64) {
    tracing::info!(
        dataset = report.dataset.name(),
        selected_files = report.files.len(),
        coverage_records = report.coverage.records,
        expected_records = report.coverage.expected_records,
        missing_time_ranges = report.coverage.missing_time_ranges_utc_ms.len(),
        unavailable_candidates = report.unavailable_sources.len(),
        elapsed_secs,
        "activity processing finished"
    );
}
