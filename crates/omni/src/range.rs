use ionoray_core::Epoch;
use serde::{Deserialize, Serialize};

use crate::OmniError;

/// Validated half-open UTC interval, at millisecond precision.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct OmniRange {
    #[serde(rename = "start_utc_ms")]
    pub(crate) start: i64,
    #[serde(rename = "end_utc_ms")]
    pub(crate) end: i64,
}

impl OmniRange {
    /// Constructs an interval; OMNI one-minute products begin in 1981.
    /// # Errors
    /// Rejects reversed, empty, non-finite, or unsupported calendar intervals.
    pub fn new(start: Epoch, end: Epoch) -> Result<Self, OmniError> {
        let first_year = start.to_gregorian_utc().0;
        let last_year = end.to_gregorian_utc().0;
        if !(1981..=9999).contains(&first_year) || !(1981..=9999).contains(&last_year) {
            return Err(OmniError::Invalid("supported UTC years: 1981..9999".into()));
        }
        let range = Self {
            start: millis(start)?,
            end: millis(end)?,
        };
        if range.start >= range.end {
            return Err(OmniError::Invalid("expected start < end".into()));
        }
        Ok(range)
    }

    /// Parses UTC start/end strings using the project's epoch parser.
    /// # Errors
    /// Returns an error for invalid epochs or intervals.
    pub fn parse(start: &str, end: &str) -> Result<Self, OmniError> {
        let parse = |value: &str| {
            value
                .parse::<Epoch>()
                .map_err(|e| OmniError::Invalid(e.to_string()))
        };
        Self::new(parse(start)?, parse(end)?)
    }

    pub(crate) fn months(self) -> Vec<Month> {
        let (year, month, ..) = epoch(self.start).to_gregorian_utc();
        let (last_year, last_month, ..) = epoch(self.end - 1).to_gregorian_utc();
        let mut year = u16::try_from(year).expect("validated calendar year");
        let mut month = month;
        let mut result = Vec::new();
        loop {
            result.push(Month { year, month });
            if i32::from(year) == last_year && month == last_month {
                break;
            }
            if month == 12 {
                year += 1;
                month = 1;
            } else {
                month += 1;
            }
        }
        result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct Month {
    pub year: u16,
    pub month: u8,
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn millis(epoch: Epoch) -> Result<i64, OmniError> {
    let value = epoch.to_unix_milliseconds();
    if !value.is_finite() {
        return Err(OmniError::Invalid("non-finite epoch".into()));
    }
    Ok(value.round() as i64)
}

#[allow(clippy::cast_precision_loss)]
pub(crate) fn epoch(millis: i64) -> Epoch {
    Epoch::from_unix_milliseconds(millis as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_only_intersecting_months() {
        let range = OmniRange::parse("2024-02-29T23:59:00Z", "2024-03-01T00:00:00Z").unwrap();
        assert_eq!(
            range.months(),
            vec![Month {
                year: 2024,
                month: 2
            }]
        );
        let range = OmniRange::parse("2023-12-31T23:59:00Z", "2024-01-01T00:01:00Z").unwrap();
        assert_eq!(range.months().len(), 2);
        assert!(OmniRange::parse("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z").is_err());
    }
}
