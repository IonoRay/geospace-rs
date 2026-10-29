use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use ionoray_core::Epoch;

use super::{ActivityDataset, ActivityError, ActivityRange, ActivityValue, range::millis};

pub(super) struct Row {
    pub time: i64,
    pub line: usize,
    pub values: BTreeMap<String, ActivityValue>,
    pub quality: String,
    pub flags: BTreeMap<String, String>,
}

pub(super) struct Parsed {
    pub rows: Vec<Row>,
    pub header: Vec<String>,
    pub source_first: i64,
    pub source_last: i64,
    pub source_records: usize,
}

pub(super) fn parse(
    path: &Path,
    dataset: ActivityDataset,
    range: ActivityRange,
    edition: &str,
) -> Result<Parsed, ActivityError> {
    let mut result = Parsed {
        rows: Vec::new(),
        header: Vec::new(),
        source_first: i64::MAX,
        source_last: i64::MIN,
        source_records: 0,
    };
    let mut previous = None;
    let mut columns = Vec::new();
    let mut quality = edition.to_owned();
    let mut data_rows = 0;
    for (index, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut row = match dataset {
            ActivityDataset::Hp30 | ActivityDataset::Hp60 => {
                if line.starts_with('#') {
                    result.header.push(line.into());
                    continue;
                }
                hpo(line, dataset)?
            }
            ActivityDataset::Pcn => {
                if !line.as_bytes()[0].is_ascii_digit() {
                    if line.to_ascii_lowercase().starts_with("data type") {
                        quality = line
                            .trim_end_matches('|')
                            .split_whitespace()
                            .skip(2)
                            .collect::<Vec<_>>()
                            .join(" ")
                            .to_ascii_lowercase();
                    }
                    if line.starts_with("DATE") || line.to_ascii_lowercase().starts_with("date,") {
                        columns = line
                            .trim_end_matches('|')
                            .split(|c: char| c.is_whitespace() || c == ',')
                            .filter(|s| !s.is_empty())
                            .map(str::to_ascii_uppercase)
                            .collect();
                    }
                    result.header.push(line.into());
                    continue;
                }
                pc(line, dataset, &columns, &quality)?
            }
        };
        if row.time.rem_euclid(dataset.cadence_seconds() * 1000) != 0
            || previous.is_some_and(|last| row.time <= last)
        {
            return Err(invalid(
                "timestamps must be strictly increasing on the native UTC grid",
            ));
        }
        previous = Some(row.time);
        result.source_first = result.source_first.min(row.time);
        result.source_last = row.time;
        result.source_records += 1;
        data_rows += 1;
        row.line = index + 1;
        if row.time >= range.start_utc_ms && row.time < range.end_utc_ms {
            result.rows.push(row);
        }
    }
    if data_rows == 0 {
        return Err(invalid("source contains no recognized records"));
    }
    if dataset == ActivityDataset::Pcn {
        if result.header.iter().any(|s| {
            s.to_ascii_lowercase().starts_with("data interval type") && !s.contains("1-min")
        }) {
            return Err(invalid("PCN source cadence is not one minute"));
        }
        if edition == "definitive" && quality != "definitive" {
            return Err(invalid(
                "definitive PCN source lacks definitive header classification",
            ));
        }
    }
    if matches!(dataset, ActivityDataset::Hp30 | ActivityDataset::Hp60)
        && (!result
            .header
            .iter()
            .any(|s| s.contains(dataset.fields()[0]))
            || !result
                .header
                .iter()
                .any(|s| s.contains("missing data indicated by -1")))
    {
        return Err(invalid(
            "Hpo source lacks its product/missingness declaration",
        ));
    }
    Ok(result)
}

pub(super) fn validate_revision(old: &Parsed, candidate: &Parsed) -> Result<(), ActivityError> {
    let times: std::collections::BTreeSet<_> = candidate.rows.iter().map(|r| r.time).collect();
    if candidate.source_first > old.source_first
        || candidate.source_last < old.source_last
        || candidate.source_records < old.source_records
        || old.rows.iter().any(|r| !times.contains(&r.time))
    {
        return Err(invalid(
            "same-source revision truncates accepted time coverage; previous input retained",
        ));
    }
    Ok(())
}

#[allow(clippy::cast_precision_loss)]
fn hpo(line: &str, dataset: ActivityDataset) -> Result<Row, ActivityError> {
    let c: Vec<_> = line.split_whitespace().collect();
    if c.len() != 10 {
        return Err(invalid("expected ten Hpo columns"));
    }
    let year = number::<i32>(c[0])?;
    let month = number::<u8>(c[1])?;
    let day = number::<u8>(c[2])?;
    let midnight = Epoch::maybe_from_gregorian_utc(year, month, day, 0, 0, 0, 0)
        .map_err(|e| invalid(&e.to_string()))?;
    let hour = number::<f64>(c[3])?;
    let mid_hour = number::<f64>(c[4])?;
    let seconds = hour * 3600.0;
    if !(0.0..24.0).contains(&hour)
        || seconds.fract() != 0.0
        || (mid_hour - hour - dataset.cadence_seconds() as f64 / 7200.0).abs() > 1e-6
    {
        return Err(invalid("invalid native Hpo interval start/midpoint"));
    }
    let time = millis(midnight)? + float_millis(seconds * 1000.0)?;
    let days = number::<f64>(c[5])?;
    let days_m = number::<f64>(c[6])?;
    let baseline = millis(
        Epoch::maybe_from_gregorian_utc(1932, 1, 1, 0, 0, 0, 0)
            .map_err(|e| invalid(&e.to_string()))?,
    )?;
    if ((days * 86_400_000.0) - (time - baseline) as f64).abs() > 500.0
        || ((days_m - days) * 86_400.0 - dataset.cadence_seconds() as f64 / 2.0).abs() > 1.0
    {
        return Err(invalid("Hpo duplicate time columns disagree"));
    }
    let fields = dataset.fields();
    Ok(Row {
        time,
        line: 0,
        values: BTreeMap::from([
            (fields[0].into(), value(number(c[7])?, -1.0)),
            (fields[1].into(), value(number(c[8])?, -1.0)),
        ]),
        quality: "unspecified_reserved_D_is_not_a_quality_flag".into(),
        flags: BTreeMap::from([
            ("D".into(), c[9].into()),
            ("mid_hour_utc".into(), c[4].into()),
        ]),
    })
}

fn pc(
    line: &str,
    dataset: ActivityDataset,
    columns: &[String],
    quality: &str,
) -> Result<Row, ActivityError> {
    let name = dataset.fields()[0];
    let position = columns
        .iter()
        .position(|s| s == name)
        .ok_or_else(|| invalid(&format!("source header does not identify {name}")))?;
    let (time, scalar) = if line.contains(',') {
        let c: Vec<_> = line.split(',').map(str::trim).collect();
        if c.len() != columns.len() {
            return Err(invalid("PC CSV column count differs from header"));
        }
        (parse_epoch(c[0])?, number::<f64>(c[position])?)
    } else {
        let c: Vec<_> = line.split_whitespace().collect();
        let scalar = c
            .get(position)
            .ok_or_else(|| invalid("truncated PC IAGA record"))?;
        if c.len() < 4 {
            return Err(invalid("truncated PC time record"));
        }
        let time = parse_epoch(&format!("{}T{}", c[0], c[1]))?;
        let doy = number::<u32>(c[2])?;
        let year = super::range::epoch(time).to_gregorian_utc().0;
        let january = millis(
            Epoch::maybe_from_gregorian_utc(year, 1, 1, 0, 0, 0, 0)
                .map_err(|e| invalid(&e.to_string()))?,
        )?;
        if i64::from(doy) != (time - january).div_euclid(86_400_000) + 1 {
            return Err(invalid("PC day-of-year disagrees with UTC date"));
        }
        (time, number::<f64>(scalar)?)
    };
    Ok(Row {
        time,
        line: 0,
        values: BTreeMap::from([(name.into(), value(scalar, 99999.0))]),
        quality: quality.into(),
        flags: BTreeMap::new(),
    })
}

pub(super) fn parse_epoch(value: &str) -> Result<i64, ActivityError> {
    let value = value.replace(' ', "T");
    let value = if value.ends_with('Z') {
        value
    } else {
        format!("{value}Z")
    };
    millis(
        value
            .parse::<Epoch>()
            .map_err(|e| invalid(&e.to_string()))?,
    )
}

// Exact upstream sentinel comparison, not an approximate physical comparison.
#[allow(clippy::float_cmp)]
fn value(raw: f64, sentinel: f64) -> ActivityValue {
    let missing = if !raw.is_finite() {
        Some("non_finite")
    } else if raw == sentinel {
        Some("fill_value")
    } else {
        None
    };
    ActivityValue {
        raw: raw.is_finite().then_some(raw),
        value: missing.is_none().then_some(raw),
        missing,
    }
}

fn number<T: std::str::FromStr>(s: &str) -> Result<T, ActivityError> {
    s.parse()
        .map_err(|_| invalid(&format!("invalid numeric source field {s:?}")))
}

#[allow(clippy::cast_possible_truncation)]
fn float_millis(value: f64) -> Result<i64, ActivityError> {
    if !value.is_finite() || value.abs() > 86_400_000.0 {
        return Err(invalid("invalid source hour"));
    }
    Ok(value.round() as i64)
}

fn invalid(message: &str) -> ActivityError {
    ActivityError::Invalid(message.into())
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
