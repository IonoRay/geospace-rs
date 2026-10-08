use std::collections::HashSet;

use ionoray_core::Epoch;

use crate::{
    parse::{IriF107Record, IriMonthlyRecord, ParsedFile, epoch_ms},
    time::epoch_millis,
};

pub(crate) fn parse_ig_rz(content: &str, target_year: u16) -> Result<ParsedFile, String> {
    let fields = content
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    if fields.len() < 9 {
        return Err("IG_RZ has an incomplete header".to_owned());
    }
    let update_month = number::<u8>(fields[0], "update month")?;
    let update_day = number::<u8>(fields[1], "update day")?;
    let update_year = number::<u16>(fields[2], "update year")?;
    let start_month = number::<u8>(fields[3], "start month")?;
    let start_year = number::<u16>(fields[4], "start year")?;
    let end_month = number::<u8>(fields[5], "end month")?;
    let end_year = number::<u16>(fields[6], "end year")?;
    validate_month(update_month)?;
    validate_date(update_year, update_month, update_day)?;
    validate_month(start_month)?;
    validate_month(end_month)?;
    if end_year < start_year {
        return Err("IG_RZ end year precedes start year".to_owned());
    }
    let start_absolute = i32::from(start_year) * 12 + i32::from(start_month) - 1;
    let end_absolute = i32::from(end_year) * 12 + i32::from(end_month) - 1;
    if end_absolute < start_absolute {
        return Err("IG_RZ end month precedes start month".to_owned());
    }
    let count = usize::try_from(end_absolute - start_absolute + 3)
        .map_err(|_| "IG_RZ month range overflow")?;
    let expected = 7 + count * 2;
    if fields.len() != expected {
        return Err(format!(
            "IG_RZ contains {} fields, expected {expected}",
            fields.len()
        ));
    }

    let (first_year, first_month) = previous_month(start_year, start_month)?;
    let prediction_start = shift_month(update_year, update_month, -6)?;
    let mut months = Vec::with_capacity(count);
    for index in 0..count {
        let month_offset = i32::try_from(index).map_err(|_| "IG_RZ month range overflow")?;
        let (year, month) = shift_month(first_year, first_month, month_offset)?;
        let ig12 = finite_number(fields[7 + index], "IG12")?;
        let mut rz12 = finite_number(fields[7 + count + index], "Rz12")?;
        if u32::from(update_year) * 100 + u32::from(update_month) > 201_609
            && u32::from(year) * 100 + u32::from(month) >= 201_401
        {
            rz12 *= 0.7;
        }
        months.push(MonthValue {
            year,
            month,
            ig12,
            rz12,
            predicted: (year, month) >= prediction_start,
        });
    }

    let mut records = Vec::new();
    for month in 1..=12 {
        for day in 1..=days_in_month(target_year, month) {
            if let Some(record) = interpolate_day(&months, target_year, month, day)? {
                records.push(record);
            }
        }
    }
    Ok(ParsedFile::IriIgRz(records))
}

pub(crate) fn parse_apf107(content: &str, target_year: u16) -> Result<ParsedFile, String> {
    let mut records = Vec::new();
    let mut dates = HashSet::new();
    let mut rows = 0_usize;
    for (line_number, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if !line.is_ascii() || bytes.len() < 54 {
            return Err(format!(
                "APF107 line {} is shorter than 54 bytes",
                line_number + 1
            ));
        }
        let raw_year = fixed::<u16>(bytes, 0, 3, "year", line_number)?;
        let year = if raw_year >= 58 {
            1900 + raw_year
        } else {
            2000 + raw_year
        };
        let month = fixed::<u8>(bytes, 3, 6, "month", line_number)?;
        let day = fixed::<u8>(bytes, 6, 9, "day", line_number)?;
        validate_date(year, month, day)?;
        if !dates.insert((year, month, day)) {
            return Err(format!(
                "APF107 contains duplicate date {year}-{month:02}-{day:02}"
            ));
        }
        rows += 1;
        let f107 = finite_fixed(bytes, 39, 44, "daily F10.7", line_number)?;
        let f107_81 = finite_fixed(bytes, 44, 49, "81-day F10.7", line_number)?;
        let f107_365 = finite_fixed(bytes, 49, 54, "365-day F10.7", line_number)?;
        if year != target_year {
            continue;
        }
        if f107 <= 0.0 {
            return Err(format!(
                "APF107 line {} has unavailable daily F10.7",
                line_number + 1
            ));
        }
        records.push(IriF107Record {
            epoch_ms: epoch_ms(year, month, day, 0)?,
            f107_adjusted: f107,
            f107a_81_adjusted: if f107_81 < -4.0 { f107 } else { f107_81 },
            f107a_365_adjusted: if f107_365 < -4.0 { f107 } else { f107_365 },
        });
    }
    if rows == 0 {
        return Err("APF107 file contains no data rows".to_owned());
    }
    Ok(ParsedFile::IriApF107(records))
}

#[derive(Clone, Copy)]
struct MonthValue {
    year: u16,
    month: u8,
    ig12: f64,
    rz12: f64,
    predicted: bool,
}

fn interpolate_day(
    months: &[MonthValue],
    year: u16,
    month: u8,
    day: u8,
) -> Result<Option<IriMonthlyRecord>, String> {
    let Some(current_index) = months
        .iter()
        .position(|value| value.year == year && value.month == month)
    else {
        return Ok(None);
    };
    let current_mid = if month == 2 { 14 } else { 15 };
    let neighbor_index = if day < current_mid {
        current_index.checked_sub(1)
    } else {
        current_index
            .checked_add(1)
            .filter(|index| *index < months.len())
    };
    let Some(neighbor_index) = neighbor_index else {
        return Ok(None);
    };
    let current = months[current_index];
    let neighbor = months[neighbor_index];
    let current_epoch = date_epoch(year, month, current_mid)?;
    let neighbor_mid = if neighbor.month == 2 { 14 } else { 15 };
    let neighbor_epoch = date_epoch(neighbor.year, neighbor.month, neighbor_mid)?;
    let query_epoch = date_epoch(year, month, day)?;
    let (left, right) = if neighbor_epoch < current_epoch {
        (neighbor, current)
    } else {
        (current, neighbor)
    };
    let (left_epoch, right_epoch) = if neighbor_epoch < current_epoch {
        (neighbor_epoch, current_epoch)
    } else {
        (current_epoch, neighbor_epoch)
    };
    let elapsed_seconds = i32::try_from((query_epoch - left_epoch) / 1_000)
        .map_err(|_| "IRI interpolation interval overflow")?;
    let span_seconds = i32::try_from((right_epoch - left_epoch) / 1_000)
        .map_err(|_| "IRI interpolation interval overflow")?;
    let weight = f64::from(elapsed_seconds) / f64::from(span_seconds);
    Ok(Some(IriMonthlyRecord {
        epoch_ms: query_epoch,
        ig12: left.ig12 + (right.ig12 - left.ig12) * weight,
        rz12: left.rz12 + (right.rz12 - left.rz12) * weight,
        quality: if left.predicted || right.predicted {
            "predicted"
        } else {
            "final"
        },
    }))
}

fn date_epoch(year: u16, month: u8, day: u8) -> Result<i64, String> {
    let epoch = Epoch::maybe_from_gregorian_utc(i32::from(year), month, day, 0, 0, 0, 0)
        .map_err(|error| error.to_string())?;
    epoch_millis(epoch).map_err(|error| error.to_string())
}

fn previous_month(year: u16, month: u8) -> Result<(u16, u8), String> {
    if month == 1 {
        Ok((
            year.checked_sub(1)
                .ok_or_else(|| "IG_RZ start year underflow".to_owned())?,
            12,
        ))
    } else {
        Ok((year, month - 1))
    }
}

fn shift_month(year: u16, month: u8, delta: i32) -> Result<(u16, u8), String> {
    let absolute = i32::from(year) * 12 + i32::from(month) - 1 + delta;
    if absolute < 0 {
        return Err("month arithmetic underflow".to_owned());
    }
    let shifted_year = u16::try_from(absolute / 12).map_err(|_| "year overflow")?;
    let shifted_month = u8::try_from(absolute % 12 + 1).map_err(|_| "month overflow")?;
    Ok((shifted_year, shifted_month))
}

fn validate_month(month: u8) -> Result<(), String> {
    if (1..=12).contains(&month) {
        Ok(())
    } else {
        Err(format!("invalid month {month}"))
    }
}

fn validate_date(year: u16, month: u8, day: u8) -> Result<(), String> {
    validate_month(month)?;
    if day == 0 || day > days_in_month(year, month) {
        return Err(format!("invalid date {year}-{month:02}-{day:02}"));
    }
    Ok(())
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

fn number<T: std::str::FromStr>(value: &str, field: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("invalid {field}"))
}

fn finite_number(value: &str, field: &str) -> Result<f64, String> {
    let value: f64 = number(value, field)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("non-finite {field}"))
    }
}

fn finite_fixed(
    bytes: &[u8],
    start: usize,
    end: usize,
    field: &str,
    line: usize,
) -> Result<f64, String> {
    let value: f64 = fixed(bytes, start, end, field, line)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("line {} has non-finite {field}", line + 1))
    }
}

fn fixed<T: std::str::FromStr>(
    bytes: &[u8],
    start: usize,
    end: usize,
    field: &str,
    line: usize,
) -> Result<T, String> {
    let value = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| format!("line {} has non-UTF-8 {field}", line + 1))?;
    value
        .trim()
        .parse()
        .map_err(|_| format!("line {} has invalid {field}", line + 1))
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    #[test]
    fn parses_and_interpolates_official_ig_rz_layout() {
        let mut ig = String::new();
        for value in 100..114 {
            write!(ig, "{value},").unwrap();
        }
        let rz = (0..14).map(|_| "100,".to_owned()).collect::<String>();
        let content = format!("8,19,2020,\n\n1,2020,12,2020,\n\n{ig}\n\n{rz}");
        let ParsedFile::IriIgRz(records) = parse_ig_rz(&content, 2020).unwrap() else {
            panic!("expected IG_RZ records");
        };
        assert_eq!(records.len(), 366);
        let january_mid = &records[14];
        assert!((january_mid.ig12 - 101.0).abs() < f64::EPSILON);
        assert!((january_mid.rz12 - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_apf107_fixed_width_values() {
        let line = format!(
            "{:3}{:3}{:3}{}{:3}{:3}{:5.1}{:5.1}{:5.1}",
            20, 1, 1, "  1  2  3  4  5  6  7  8", 5, -11, 70.0, 71.0, 72.0
        );
        let ParsedFile::IriApF107(records) = parse_apf107(&line, 2020).unwrap() else {
            panic!("expected APF107 records");
        };
        assert_eq!(records.len(), 1);
        assert!((records[0].f107a_81_adjusted - 71.0).abs() < f64::EPSILON);
    }

    #[test]
    fn apf107_accepts_partial_target_year_and_rejects_duplicates() {
        let line = format!(
            "{:3}{:3}{:3}{}{:3}{:3}{:5.1}{:5.1}{:5.1}",
            20, 2, 29, "  1  2  3  4  5  6  7  8", 5, -11, 70.0, 71.0, 72.0
        );
        let ParsedFile::IriApF107(records) = parse_apf107(&line, 2020).unwrap() else {
            panic!("expected APF107 records");
        };
        assert_eq!(records.len(), 1);
        assert!(parse_apf107(&format!("{line}\n{line}"), 2020).is_err());
    }

    #[test]
    fn rejects_non_finite_ig_rz_values() {
        let content = "8,19,2020,1,2020,1,2020,NaN,1,1,1";
        assert!(parse_ig_rz(content, 2020).is_err());
    }
}
