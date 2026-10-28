//! Physical source partition keys and declared UTC coverage.
use crate::{IndexError, ValidatedIndexFile, time::epoch_millis};
use ionoray_core::Epoch;

pub(super) fn partition_key(file: &ValidatedIndexFile) -> String {
    match (file.month, file.day) {
        (Some(month), Some(day)) => format!("day-{month:02}-{day:02}"),
        (Some(month), None) => format!("month-{month:02}"),
        _ => "rolling".to_owned(),
    }
}

pub(super) fn coverage(file: &ValidatedIndexFile) -> Result<(i64, i64), IndexError> {
    let year = file.year;
    let month = file.month.unwrap_or(1);
    let start =
        Epoch::maybe_from_gregorian_utc(i32::from(year), month, file.day.unwrap_or(1), 0, 0, 0, 0)
            .map_err(|_| IndexError::InvalidNumber("coverage start"))?;
    let start = epoch_millis(start)?;
    if file.day.is_some() {
        return Ok((start, start + 86_400_000));
    }
    let (end_year, end_month) = match file.month {
        None | Some(12) => (
            year.checked_add(1)
                .ok_or(IndexError::InvalidNumber("coverage end year"))?,
            1,
        ),
        Some(month) => (year, month + 1),
    };
    let end = Epoch::maybe_from_gregorian_utc(i32::from(end_year), end_month, 1, 0, 0, 0, 0)
        .map_err(|_| IndexError::InvalidNumber("coverage end"))?;
    Ok((start, epoch_millis(end)?))
}
