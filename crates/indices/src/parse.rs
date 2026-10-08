use std::{collections::HashSet, path::Path};

use ionoray_core::Epoch;

use crate::{
    IndexDataset, IndexError,
    f107::{DailyF107Input, normalize},
    time::epoch_millis,
};

#[derive(Clone, Debug)]
pub(crate) struct GeomagneticRecord {
    pub(crate) epoch_ms: i64,
    pub(crate) kp_thirds: Option<i32>,
    pub(crate) ap: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct DailyRecord {
    pub(crate) epoch_ms: i64,
    pub(crate) ap_daily: Option<f64>,
    pub(crate) sunspot_number: Option<f64>,
    pub(crate) f107_observed: Option<f64>,
    pub(crate) f107_observed_interpolation_gap_days: u16,
    pub(crate) f107_adjusted: Option<f64>,
    pub(crate) f107a: Option<f64>,
    pub(crate) f107a_interpolated_input_count: u16,
    pub(crate) quality: String,
}

#[derive(Clone, Debug)]
pub(crate) struct HourlyRecord {
    pub(crate) epoch_ms: i64,
    /// `None` is an upstream-declared missing sample, distinct from no key.
    pub(crate) value: Option<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct IriMonthlyRecord {
    pub(crate) epoch_ms: i64,
    pub(crate) ig12: f64,
    pub(crate) rz12: f64,
    pub(crate) quality: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct IriF107Record {
    pub(crate) epoch_ms: i64,
    pub(crate) f107_adjusted: f64,
    pub(crate) f107a_81_adjusted: f64,
    pub(crate) f107a_365_adjusted: f64,
}

#[derive(Clone, Debug)]
pub(crate) enum ParsedFile {
    Gfz {
        geomagnetic: Vec<GeomagneticRecord>,
        daily: Vec<DailyRecord>,
    },
    Dst(Vec<HourlyRecord>),
    Ae(Vec<HourlyRecord>),
    IriIgRz(Vec<IriMonthlyRecord>),
    IriApF107(Vec<IriF107Record>),
}

pub(crate) async fn parse_file(
    dataset: IndexDataset,
    path: &Path,
    year: u16,
    month: Option<u8>,
) -> Result<ParsedFile, IndexError> {
    let content =
        tokio::fs::read_to_string(path)
            .await
            .map_err(|source| IndexError::ReadArtifact {
                path: path.to_path_buf(),
                source,
            })?;
    match dataset {
        IndexDataset::KpApF107 => parse_gfz(&content, year),
        IndexDataset::Dst => {
            required_month(month).and_then(|month| parse_dst(&content, year, month))
        }
        IndexDataset::Ae => required_month(month).and_then(|month| parse_ae(&content, year, month)),
        IndexDataset::IriIgRz => crate::parse_iri::parse_ig_rz(&content, year),
        IndexDataset::IriApF107 => crate::parse_iri::parse_apf107(&content, year),
    }
    .map_err(|reason| IndexError::Validation {
        dataset,
        path: path.to_path_buf(),
        reason,
    })
}

fn parse_gfz(content: &str, target_year: u16) -> Result<ParsedFile, String> {
    let mut rows = Vec::new();
    let mut dates = HashSet::new();
    for (line_number, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 27 {
            return Err(format!("line {} has fewer than 27 fields", line_number + 1));
        }
        let year = number(fields[0], "GFZ year", line_number)?;
        let month = number(fields[1], "GFZ month", line_number)?;
        let day = number(fields[2], "GFZ day", line_number)?;
        validate_date(year, month, day)?;
        if !dates.insert((year, month, day)) {
            return Err(format!("duplicate GFZ date {year}-{month:02}-{day:02}"));
        }
        let mut kp = [0.0; 8];
        let mut ap = [0.0; 8];
        for index in 0..8 {
            kp[index] = finite_number(fields[7 + index], "GFZ Kp", line_number)?;
            ap[index] = finite_number(fields[15 + index], "GFZ ap", line_number)?;
        }
        rows.push(GfzRow {
            year,
            month,
            day,
            kp,
            ap,
            ap_daily: finite_number(fields[23], "GFZ Ap", line_number)?,
            sunspot: optional_value(fields[24], line_number)?,
            f107_observed: optional_value(fields[25], line_number)?,
            f107_adjusted: optional_value(fields[26], line_number)?,
            quality: fields.get(27).copied().unwrap_or("unknown").to_owned(),
        });
    }
    if rows.is_empty() {
        return Err("GFZ file contains no data rows".to_owned());
    }
    let inputs = rows
        .iter()
        .map(|row| DailyF107Input {
            year: row.year,
            month: row.month,
            day: row.day,
            observed: row.f107_observed,
        })
        .collect::<Vec<_>>();
    let (normalized, averages) = normalize(&inputs, target_year);
    let mut geomagnetic = Vec::new();
    let mut daily = Vec::new();
    for (row_index, row) in rows.into_iter().enumerate() {
        if row.year != target_year {
            continue;
        }
        let day_start = epoch_ms(row.year, row.month, row.day, 0)?;
        for interval in 0_u8..8 {
            let index = usize::from(interval);
            geomagnetic.push(GeomagneticRecord {
                epoch_ms: day_start + i64::from(interval) * 3 * 60 * 60 * 1_000,
                kp_thirds: (row.kp[index] >= 0.0)
                    .then(|| kp_thirds(row.kp[index]))
                    .transpose()?,
                ap: (row.ap[index] >= 0.0).then_some(row.ap[index]),
            });
        }
        daily.push(DailyRecord {
            epoch_ms: day_start,
            ap_daily: (row.ap_daily >= 0.0).then_some(row.ap_daily),
            sunspot_number: row.sunspot,
            f107_observed: normalized[row_index].map(|value| value.value),
            f107_observed_interpolation_gap_days: normalized[row_index]
                .map_or(0, |value| value.interpolation_gap_days),
            f107_adjusted: row.f107_adjusted,
            f107a: averages[row_index].map(|value| value.value),
            f107a_interpolated_input_count: averages[row_index]
                .map_or(0, |value| value.interpolated_input_count),
            quality: row.quality,
        });
    }
    Ok(ParsedFile::Gfz { geomagnetic, daily })
}

fn parse_dst(content: &str, year: u16, month: u8) -> Result<ParsedFile, String> {
    let mut values = Vec::new();
    let mut dates = HashSet::new();
    for (line_number, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bytes = line.as_bytes();
        if !line.is_ascii() || bytes.len() < 120 || !line.starts_with("DST") {
            return Err(format!("line {} is not WDC-like Dst", line_number + 1));
        }
        let row_year = fixed::<u16>(bytes, 14, 16, "Dst century", line_number)?
            .checked_mul(100)
            .and_then(|century| {
                fixed::<u16>(bytes, 3, 5, "Dst year", line_number)
                    .ok()
                    .and_then(|short| century.checked_add(short))
            })
            .ok_or_else(|| format!("line {} has invalid Dst year", line_number + 1))?;
        let row_month: u8 = fixed(bytes, 5, 7, "Dst month", line_number)?;
        let day: u8 = fixed(bytes, 8, 10, "Dst day", line_number)?;
        if row_year != year || row_month != month {
            return Err(format!(
                "line {} contains {row_year}-{row_month:02}, expected {year}-{month:02}",
                line_number + 1
            ));
        }
        validate_date(year, month, day)?;
        if !dates.insert(day) {
            return Err(format!("duplicate Dst date {year}-{month:02}-{day:02}"));
        }
        let base: i32 = fixed(bytes, 16, 20, "Dst base", line_number)?;
        for hour in 0..24_u8 {
            let start = 20 + usize::from(hour) * 4;
            let residual: i32 = fixed(bytes, start, start + 4, "Dst value", line_number)?;
            let value = if residual == 9_999 {
                None
            } else {
                Some(
                    base.checked_mul(100)
                        .and_then(|value| value.checked_add(residual))
                        .ok_or_else(|| format!("line {} Dst value overflows", line_number + 1))?,
                )
            };
            values.push(HourlyRecord {
                epoch_ms: epoch_ms(year, month, day, hour)?,
                value: value.map(f64::from),
            });
        }
    }
    if dates.is_empty() {
        return Err("Dst file contains no data rows".to_owned());
    }
    Ok(ParsedFile::Dst(values))
}

fn parse_ae(content: &str, year: u16, month: u8) -> Result<ParsedFile, String> {
    let mut values = Vec::new();
    let mut hours = HashSet::new();
    for (line_number, line) in content.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('[') {
            continue;
        }
        let bytes = line.as_bytes();
        if !line.is_ascii()
            || bytes.len() < 400
            || !line.starts_with("AEALAOAU")
            || bytes[18] != b'E'
        {
            return Err(format!("line {} is not Kyoto E02 AE", line_number + 1));
        }
        let short_year: u16 = fixed(bytes, 12, 14, "AE year", line_number)?;
        let row_month: u8 = fixed(bytes, 14, 16, "AE month", line_number)?;
        let day: u8 = fixed(bytes, 16, 18, "AE day", line_number)?;
        let hour: u8 = fixed(bytes, 19, 21, "AE hour", line_number)?;
        if short_year != year % 100 || row_month != month {
            return Err(format!(
                "line {} does not contain requested AE period",
                line_number + 1
            ));
        }
        validate_date(year, month, day)?;
        if hour > 23 {
            return Err(format!("line {} has invalid AE hour", line_number + 1));
        }
        if !hours.insert((day, hour)) {
            return Err(format!(
                "duplicate AE hour {year}-{month:02}-{day:02}T{hour:02}"
            ));
        }
        for offset in (34..400).step_by(6) {
            let _: i32 = fixed(bytes, offset, offset + 6, "AE value", line_number)?;
        }
        let value: i32 = fixed(bytes, 394, 400, "AE hourly mean", line_number)?;
        values.push(HourlyRecord {
            epoch_ms: epoch_ms(year, month, day, hour)?,
            value: (value != 99_999).then(|| f64::from(value)),
        });
    }
    if hours.is_empty() {
        return Err("AE file contains no hourly records".to_owned());
    }
    Ok(ParsedFile::Ae(values))
}

struct GfzRow {
    year: u16,
    month: u8,
    day: u8,
    kp: [f64; 8],
    ap: [f64; 8],
    ap_daily: f64,
    sunspot: Option<f64>,
    f107_observed: Option<f64>,
    f107_adjusted: Option<f64>,
    quality: String,
}

fn optional_value(value: &str, line: usize) -> Result<Option<f64>, String> {
    let value = finite_number(value, "optional GFZ value", line)?;
    Ok((value >= 0.0).then_some(value))
}

fn required_month(month: Option<u8>) -> Result<u8, String> {
    month.ok_or_else(|| "monthly index source is missing its requested month".to_owned())
}

pub(crate) fn epoch_ms(year: u16, month: u8, day: u8, hour: u8) -> Result<i64, String> {
    let epoch = Epoch::maybe_from_gregorian_utc(i32::from(year), month, day, hour, 0, 0, 0)
        .map_err(|error| error.to_string())?;
    epoch_millis(epoch).map_err(|error| error.to_string())
}

#[allow(clippy::cast_possible_truncation)]
fn kp_thirds(kp: f64) -> Result<i32, String> {
    let thirds = (kp * 3.0).round();
    if !(0.0..=27.0).contains(&thirds) {
        return Err(format!("Kp {kp} is outside the supported range"));
    }
    Ok(thirds as i32)
}

fn number<T: std::str::FromStr>(value: &str, field: &str, line: usize) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("line {} has invalid {field}", line + 1))
}

fn finite_number(value: &str, field: &str, line: usize) -> Result<f64, String> {
    let value: f64 = number(value, field, line)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("line {} has non-finite {field}", line + 1))
    }
}

fn validate_date(year: u16, month: u8, day: u8) -> Result<(), String> {
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
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

fn fixed<T: std::str::FromStr>(
    bytes: &[u8],
    start: usize,
    end: usize,
    field: &str,
    line: usize,
) -> Result<T, String> {
    let value = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| format!("line {} has non-UTF-8 {field}", line + 1))?;
    number(value.trim(), field, line)
}

#[cfg(test)]
mod tests;
