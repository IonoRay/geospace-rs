//! Field-level coverage auditing for bounded synchronization.

use std::collections::HashSet;

use ionoray_store::StoreScope;
use turso::params;

use crate::{
    IndexDataset, IndexError,
    database::open_existing_year,
    range::{CoverageGap, CoverageGapReason, FieldCoverage, IndexField},
};

const HOUR_MS: i64 = 60 * 60 * 1_000;
const THREE_HOUR_MS: i64 = 3 * HOUR_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

pub(crate) async fn audit(
    store: &ionoray_store::StoreRoot,
    dataset: IndexDataset,
    fields: &[IndexField],
    start_utc_ms: i64,
    end_utc_ms: i64,
    absent_reason: CoverageGapReason,
) -> Result<Vec<FieldCoverage>, IndexError> {
    let scoped = store.scoped(StoreScope::new(dataset.scope_name())?).await?;
    let _guard = scoped.try_acquire_read_guard()?;
    let mut result = Vec::with_capacity(fields.len());
    for field in fields {
        result.push(
            audit_field(
                store,
                dataset,
                *field,
                start_utc_ms,
                end_utc_ms,
                absent_reason,
            )
            .await?,
        );
    }
    Ok(result)
}

async fn audit_field(
    store: &ionoray_store::StoreRoot,
    dataset: IndexDataset,
    field: IndexField,
    start: i64,
    end: i64,
    absent_reason: CoverageGapReason,
) -> Result<FieldCoverage, IndexError> {
    let (table, timestamp, value, step) = source(field);
    let query_start = align_down(start, step);
    let years = years(query_start, end)?;
    let mut present = HashSet::new();
    for year in years {
        let path = store
            .layout()
            .index_database(dataset.provider(), dataset.as_str(), year);
        if !path.exists() {
            continue;
        }
        let database = open_existing_year(path, dataset, year).await?;
        let connection = database.database.connect()?;
        let statement = format!(
            "SELECT DISTINCT d.{timestamp} FROM {table} d JOIN active_release active ON active.release_id = d.release_id WHERE d.{timestamp} >= ? AND d.{timestamp} < ? AND d.{value} IS NOT NULL"
        );
        let mut rows = connection
            .query(&statement, params![query_start, end])
            .await?;
        while let Some(row) = rows.next().await? {
            present.insert(row.get::<i64>(0)?);
        }
    }
    let gaps = gaps(field, start, end, step, &present, absent_reason);
    Ok(FieldCoverage {
        field,
        requested_start_utc_ms: start,
        requested_end_utc_ms: end,
        available_samples: present.len(),
        gaps,
    })
}

fn source(field: IndexField) -> (&'static str, &'static str, &'static str, i64) {
    match field {
        IndexField::Kp => (
            "geomagnetic_3h",
            "interval_start_utc_ms",
            "kp_thirds",
            THREE_HOUR_MS,
        ),
        IndexField::Ap3h => (
            "geomagnetic_3h",
            "interval_start_utc_ms",
            "ap",
            THREE_HOUR_MS,
        ),
        IndexField::DailyAp => ("space_weather_daily", "date_utc_ms", "ap_daily", DAY_MS),
        IndexField::F107 => (
            "space_weather_daily",
            "date_utc_ms",
            "f107_observed_sfu",
            DAY_MS,
        ),
        IndexField::F107a => (
            "space_weather_daily",
            "date_utc_ms",
            "f107a_81d_sfu",
            DAY_MS,
        ),
        IndexField::Dst => ("dst_hourly", "epoch_utc_ms", "dst_nt", HOUR_MS),
        IndexField::Ae => ("ae_hourly", "epoch_utc_ms", "ae_nt", HOUR_MS),
        IndexField::Ig12 => ("iri_ig_rz_daily", "date_utc_ms", "ig12", DAY_MS),
        IndexField::Rz12 => ("iri_ig_rz_daily", "date_utc_ms", "rz12", DAY_MS),
        IndexField::IriF107 => ("iri_f107_daily", "date_utc_ms", "f107_adjusted_sfu", DAY_MS),
        IndexField::IriF107a81 => (
            "iri_f107_daily",
            "date_utc_ms",
            "f107a_81d_adjusted_sfu",
            DAY_MS,
        ),
        IndexField::IriF107a365 => (
            "iri_f107_daily",
            "date_utc_ms",
            "f107a_365d_adjusted_sfu",
            DAY_MS,
        ),
    }
}

fn gaps(
    field: IndexField,
    start: i64,
    end: i64,
    step: i64,
    present: &HashSet<i64>,
    reason: CoverageGapReason,
) -> Vec<CoverageGap> {
    let mut result = Vec::new();
    let mut cursor = align_down(start, step);
    let mut missing_start: Option<i64> = None;
    while cursor < end {
        if present.contains(&cursor) {
            if let Some(gap_start) = missing_start.take() {
                result.push(CoverageGap {
                    field,
                    start_utc_ms: gap_start.max(start),
                    end_utc_ms: cursor.min(end),
                    reason,
                });
            }
        } else if missing_start.is_none() {
            missing_start = Some(cursor);
        }
        cursor = cursor.saturating_add(step);
    }
    if let Some(gap_start) = missing_start {
        result.push(CoverageGap {
            field,
            start_utc_ms: gap_start.max(start),
            end_utc_ms: end,
            reason,
        });
    }
    result
}

fn align_down(value: i64, step: i64) -> i64 {
    value.div_euclid(step) * step
}

fn years(start: i64, end: i64) -> Result<Vec<u16>, IndexError> {
    let first = crate::time::epoch_from_millis(start).to_gregorian_utc().0;
    let last = crate::time::epoch_from_millis(end.saturating_sub(1))
        .to_gregorian_utc()
        .0;
    (first..=last)
        .map(|year| u16::try_from(year).map_err(|_| IndexError::InvalidNumber("range year")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_missing_samples_are_merged() {
        let present = HashSet::from([0, 3]);
        let gaps = gaps(
            IndexField::Dst,
            0,
            5,
            1,
            &present,
            CoverageGapReason::LocalMissing,
        );
        assert_eq!(gaps.len(), 2);
        assert_eq!((gaps[0].start_utc_ms, gaps[0].end_utc_ms), (1, 3));
        assert_eq!((gaps[1].start_utc_ms, gaps[1].end_utc_ms), (4, 5));
    }
}
