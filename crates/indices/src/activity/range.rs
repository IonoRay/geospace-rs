use ionoray_core::Epoch;
use serde::Serialize;

use super::ActivityError;

/// Validated UTC half-open interval `[start,end)`.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ActivityRange {
    /// Inclusive UTC Unix milliseconds.
    pub(super) start_utc_ms: i64,
    /// Exclusive UTC Unix milliseconds.
    pub(super) end_utc_ms: i64,
}

impl ActivityRange {
    /// Accepts ISO UTC epochs or date-only values (midnight UTC).
    /// # Errors
    /// Rejects invalid, empty, reversed, or unsupported calendar intervals.
    pub fn parse(start: &str, end: &str) -> Result<Self, ActivityError> {
        let parse = |value: &str| {
            let value = if value.len() == 10 {
                format!("{value}T00:00:00Z")
            } else {
                value.into()
            };
            value
                .parse::<Epoch>()
                .map_err(|e| ActivityError::Invalid(e.to_string()))
        };
        Self::new(parse(start)?, parse(end)?)
    }

    /// Creates a range from explicit epochs.
    /// # Errors
    /// Rejects unsupported or empty intervals.
    pub fn new(start: Epoch, end: Epoch) -> Result<Self, ActivityError> {
        if !(1900..=9999).contains(&start.to_gregorian_utc().0)
            || !(1900..=9999).contains(&end.to_gregorian_utc().0)
        {
            return Err(ActivityError::Invalid(
                "supported UTC years: 1900..9999".into(),
            ));
        }
        let range = Self {
            start_utc_ms: millis(start)?,
            end_utc_ms: millis(end)?,
        };
        if range.start_utc_ms >= range.end_utc_ms {
            return Err(ActivityError::Invalid("expected start < end".into()));
        }
        Ok(range)
    }

    pub(super) fn days(self) -> Vec<(u16, u8, u8)> {
        let mut result = Vec::new();
        let mut day = self.start_utc_ms.div_euclid(86_400_000) * 86_400_000;
        while day < self.end_utc_ms {
            let (year, month, date, ..) = epoch(day).to_gregorian_utc();
            result.push((
                u16::try_from(year).expect("validated calendar"),
                month,
                date,
            ));
            day += 86_400_000;
        }
        result
    }
}

pub(super) fn millis(epoch: Epoch) -> Result<i64, ActivityError> {
    crate::time::epoch_millis(epoch).map_err(|e| ActivityError::Invalid(e.to_string()))
}

pub(super) fn epoch(millis: i64) -> Epoch {
    crate::time::epoch_from_millis(millis)
}
