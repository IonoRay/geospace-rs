use ionoray_core::{Duration, QueryPoint};
use ionoray_indices::{Ap, F107, IndexDataset, IndexField, IndexSample, ValueDerivation};
use ionoray_msis::{
    Msis, MsisApHistory, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisResult, MsisVersion,
};
use serde::{Deserialize, Serialize};

use crate::{DataPolicy, Geospace, GeospaceError};

const AP_HISTORY_HOURS: [u8; 20] = [
    0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45, 48, 51, 54, 57,
];

/// Optional NRLMSIS drivers: omitted fields are resolved at preparation or inherited in scenarios.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MsisDriverOverrides {
    /// Explicit centered 81-day F10.7 average.
    pub f107a: Option<f64>,
    /// Explicit previous-day F10.7.
    pub f107_previous_day: Option<f64>,
    /// Explicit daily-Ap or storm-time geomagnetic formulation.
    pub geomagnetic_activity: Option<MsisGeomagneticActivity>,
}

/// High-level NRLMSIS request defined by spacetime and optional driver overrides.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisRequest {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Explicit fields that take precedence over index-derived values.
    pub overrides: MsisDriverOverrides,
}

impl MsisRequest {
    /// Creates a request whose complete environmental input comes from indices.
    pub const fn new(query: QueryPoint) -> Self {
        Self {
            query,
            overrides: MsisDriverOverrides {
                f107a: None,
                f107_previous_day: None,
                geomagnetic_activity: None,
            },
        }
    }
}

/// Index samples consumed while resolving an NRLMSIS request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MsisIndexEvidence {
    /// Current-day centered 81-day F10.7 sample, absent when explicitly overridden.
    pub f107a: Option<IndexSample<F107>>,
    /// Previous-day observed F10.7 sample, absent when explicitly overridden.
    pub f107_previous_day: Option<IndexSample<F107>>,
    /// Current daily Ap sample, absent when geomagnetic activity is overridden.
    pub ap_daily: Option<IndexSample<Ap>>,
    /// Three-hour ap samples ordered from current through 57 hours earlier.
    pub ap_three_hourly: Vec<IndexSample<Ap>>,
}

/// Reusable NRLMSIS input prepared from explicit and indexed drivers.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedMsis {
    /// Exact low-level model input that will be evaluated.
    input: MsisInput,
    /// Index evidence for every automatically resolved driver.
    indices: MsisIndexEvidence,
}

/// NRLMSIS output paired with its exact resolved input and index evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisEvaluation {
    /// Exact input evaluated by NRLMSIS 2.1.
    pub input: MsisInput,
    /// Index evidence for every automatically resolved driver.
    pub indices: MsisIndexEvidence,
    /// Scientific model result and provenance.
    pub result: MsisResult,
}

impl Geospace {
    /// Resolves NRLMSIS input under an explicit data-access policy.
    ///
    /// Missing activity requires the complete storm-time history, never a constant Ap fallback.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when required index data cannot be prepared or read.
    pub async fn prepare_msis(
        &self,
        request: MsisRequest,
        policy: DataPolicy,
    ) -> Result<PreparedMsis, GeospaceError> {
        let mut stage = crate::runner::StageTrace::new("model.prepare", "nrlmsis21");
        let span = stage.span();
        tracing::Instrument::instrument(
            async {
                let need_current = request.overrides.f107a.is_none()
                    || request.overrides.geomagnetic_activity.is_none();
                let need_previous = request.overrides.f107_previous_day.is_none();
                let need_ap_history = request.overrides.geomagnetic_activity.is_none();
                let previous_epoch = request.query.epoch - Duration::from_hours(24.0);

                let mut epochs = Vec::new();
                if need_current {
                    epochs.push(request.query.epoch);
                }
                if need_previous {
                    epochs.push(previous_epoch);
                }
                if need_ap_history {
                    epochs.extend(AP_HISTORY_HOURS.iter().skip(1).map(|hours| {
                        request.query.epoch - Duration::from_hours(f64::from(*hours))
                    }));
                }
                self.ensure_space_weather_epochs(
                    &epochs,
                    required_fields(request.overrides),
                    policy,
                )
                .await?;

                let mut evidence = MsisIndexEvidence::default();
                let f107a = if let Some(value) = request.overrides.f107a {
                    value
                } else {
                    let sample = self.indices().f107a_at(request.query.epoch).await?;
                    self.warn_interpolated_driver("f107a", &sample);
                    let value = sample.value.value();
                    evidence.f107a = Some(sample);
                    value
                };
                let f107_previous_day = if let Some(value) = request.overrides.f107_previous_day {
                    value
                } else {
                    let sample = self.indices().f107_at(previous_epoch).await?;
                    self.warn_interpolated_driver("f107_previous_day", &sample);
                    let value = sample.value.value();
                    evidence.f107_previous_day = Some(sample);
                    value
                };
                let geomagnetic_activity = if let Some(value) =
                    request.overrides.geomagnetic_activity
                {
                    value
                } else {
                    evidence.ap_daily =
                        Some(self.indices().daily_ap_at(request.query.epoch).await?);
                    evidence
                        .ap_three_hourly
                        .push(self.indices().ap_at(request.query.epoch).await?);
                    for hours in AP_HISTORY_HOURS.iter().skip(1) {
                        evidence.ap_three_hourly.push(
                            self.indices()
                                .ap_at(
                                    request.query.epoch - Duration::from_hours(f64::from(*hours)),
                                )
                                .await?,
                        );
                    }
                    MsisGeomagneticActivity::StormTime(msis_ap_history(&evidence)?)
                };

                let prepared = PreparedMsis {
                    input: MsisInput {
                        query: request.query,
                        drivers: MsisDrivers {
                            f107a,
                            f107_previous_day,
                            geomagnetic_activity,
                        },
                    },
                    indices: evidence,
                };
                let sample_count = usize::from(prepared.indices.f107a.is_some())
                    + usize::from(prepared.indices.f107_previous_day.is_some())
                    + usize::from(prepared.indices.ap_daily.is_some())
                    + prepared.indices.ap_three_hourly.len();
                stage.succeeded(
                    sample_count,
                    if sample_count == 0 {
                        "explicit"
                    } else {
                        "indices_or_mixed"
                    },
                );
                Ok(prepared)
            },
            span,
        )
        .await
    }

    /// Resolves missing NRLMSIS drivers and evaluates NRLMSIS 2.1.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] for data preparation or model evaluation failures.
    pub async fn evaluate_msis(
        &self,
        request: MsisRequest,
        policy: DataPolicy,
    ) -> Result<MsisEvaluation, GeospaceError> {
        self.prepare_msis(request, policy).await?.evaluate()
    }

    async fn ensure_space_weather_epochs(
        &self,
        epochs: &[ionoray_core::Epoch],
        fields: Vec<IndexField>,
        policy: DataPolicy,
    ) -> Result<(), GeospaceError> {
        let Some(start) = epochs.iter().min().copied() else {
            return Ok(());
        };
        let end = epochs.iter().max().copied().expect("nonempty epochs")
            + Duration::from_milliseconds(1.0);
        let result = self
            .prepare_fields_range(IndexDataset::KpApF107, fields, start, end, policy)
            .await;
        // The shared source is prepared once for the union of driver times.
        // Each required sample is read explicitly afterwards; gaps at other
        // times in this union must not reject an otherwise usable model input.
        match result {
            Err(GeospaceError::PartialIndices(report))
                if report.source_check_status != ionoray_indices::SourceCheckStatus::Failed =>
            {
                tracing::warn!(diagnostic = %report.diagnostic(), "driver interval has gaps; checking exact required samples");
                Ok(())
            }
            result => result,
        }
    }
}

fn required_fields(overrides: MsisDriverOverrides) -> Vec<IndexField> {
    let mut fields = Vec::new();
    if overrides.f107a.is_none() {
        fields.push(IndexField::F107a);
    }
    if overrides.f107_previous_day.is_none() {
        fields.push(IndexField::F107);
    }
    if overrides.geomagnetic_activity.is_none() {
        fields.extend([IndexField::DailyAp, IndexField::Ap3h]);
    }
    fields
}

impl Geospace {
    fn warn_interpolated_driver(&self, driver: &'static str, sample: &IndexSample<F107>) {
        let uses_interpolation = matches!(
            sample.derivation,
            ValueDerivation::LinearInterpolation { .. }
                | ValueDerivation::CenteredMean {
                    interpolated_input_count: 1..,
                    ..
                }
        );
        if !uses_interpolation
            || !self.mark_adjustment_warning(format!(
                "nrlmsis21|{driver}|{}|{}|{}",
                sample.interval.start, sample.interval.end, sample.release_id
            ))
        {
            return;
        }
        match sample.derivation {
            ValueDerivation::LinearInterpolation { gap_days } => tracing::warn!(
                event = "geospace.msis.driver.interpolated",
                model = "nrlmsis21",
                driver,
                gap_days,
                interval_start = %sample.interval.start,
                interval_end = %sample.interval.end,
                release_id = %sample.release_id,
                artifact_sha256 = %sample.artifact,
                "NRLMSIS driver uses a linearly interpolated F10.7 observation"
            ),
            ValueDerivation::CenteredMean {
                window_days,
                interpolated_input_count,
            } if interpolated_input_count > 0 => tracing::warn!(
                event = "geospace.msis.driver.interpolated",
                model = "nrlmsis21",
                driver,
                window_days,
                interpolated_input_count,
                interval_start = %sample.interval.start,
                interval_end = %sample.interval.end,
                release_id = %sample.release_id,
                artifact_sha256 = %sample.artifact,
                "NRLMSIS aggregate driver includes linearly interpolated F10.7 observations"
            ),
            ValueDerivation::Source | ValueDerivation::CenteredMean { .. } => {}
        }
    }
}

impl PreparedMsis {
    /// Copies this baseline and replaces only explicitly supplied drivers.
    ///
    /// `None` inherits input and evidence. `Some`, even the same value, clears
    /// the corresponding evidence. Activity replaces the whole formulation and
    /// clears both daily and three-hourly ap evidence. No indices are read;
    /// validation occurs at evaluation.
    #[must_use]
    pub fn with_overrides(&self, overrides: MsisDriverOverrides) -> Self {
        let mut scenario = self.clone();
        if let Some(value) = overrides.f107a {
            scenario.input.drivers.f107a = value;
            scenario.indices.f107a = None;
        }
        if let Some(value) = overrides.f107_previous_day {
            scenario.input.drivers.f107_previous_day = value;
            scenario.indices.f107_previous_day = None;
        }
        if let Some(value) = overrides.geomagnetic_activity {
            scenario.input.drivers.geomagnetic_activity = value;
            scenario.indices.ap_daily = None;
            scenario.indices.ap_three_hourly.clear();
        }
        scenario
    }

    /// Returns the exact resolved model input.
    pub const fn input(&self) -> &MsisInput {
        &self.input
    }

    /// Returns the evidence for automatically resolved drivers.
    pub const fn indices(&self) -> &MsisIndexEvidence {
        &self.indices
    }

    /// Executes the resolved NRLMSIS input without store or network access.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when model evaluation fails.
    pub fn evaluate(&self) -> Result<MsisEvaluation, GeospaceError> {
        let result = Msis::new(MsisVersion::Nrlmsis21).evaluate(&self.input)?;
        Ok(MsisEvaluation {
            input: self.input,
            indices: self.indices.clone(),
            result,
        })
    }
}

fn msis_ap_history(evidence: &MsisIndexEvidence) -> Result<MsisApHistory, GeospaceError> {
    let values: Vec<f64> = evidence
        .ap_three_hourly
        .iter()
        .map(|sample| sample.value.value())
        .collect();
    if values.len() != AP_HISTORY_HOURS.len() {
        return Err(GeospaceError::InvalidMsisPreparation(
            "expected 20 three-hour ap samples",
        ));
    }
    let daily = evidence
        .ap_daily
        .as_ref()
        .ok_or(GeospaceError::InvalidMsisPreparation("missing daily Ap"))?
        .value
        .value();
    Ok(MsisApHistory {
        daily,
        current: values[0],
        three_hours_ago: values[1],
        six_hours_ago: values[2],
        nine_hours_ago: values[3],
        average_12_to_33_hours: mean(&values[4..12]),
        average_36_to_57_hours: mean(&values[12..20]),
    })
}

fn mean(values: &[f64]) -> f64 {
    let count = u32::try_from(values.len()).expect("NRLMSIS averaging windows fit in u32");
    values.iter().sum::<f64>() / f64::from(count)
}

#[cfg(test)]
mod tests {
    use ionoray_core::{Epoch, GeodeticPosition, Sha256Digest};
    use ionoray_indices::{QualityFlag, TimeInterval, ValueDerivation};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn builds_official_storm_time_ap_windows() {
        let epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap();
        let sample = |value| IndexSample {
            value: Ap::new(value).unwrap(),
            interval: TimeInterval {
                start: epoch,
                end: epoch + Duration::from_hours(3.0),
            },
            quality: QualityFlag::Final,
            derivation: ValueDerivation::Source,
            release_id: "test".to_owned(),
            artifact: Sha256Digest::from_bytes([0; 32]),
            snapshot: Sha256Digest::from_bytes([0; 32]),
        };
        let evidence = MsisIndexEvidence {
            ap_daily: Some(sample(7.0)),
            ap_three_hourly: (0..20).map(|value| sample(f64::from(value))).collect(),
            ..MsisIndexEvidence::default()
        };
        let history = msis_ap_history(&evidence).unwrap();
        assert_close(history.daily, 7.0);
        assert_close(history.current, 0.0);
        assert_close(history.nine_hours_ago, 3.0);
        assert_close(history.average_12_to_33_hours, 7.5);
        assert_close(history.average_36_to_57_hours, 15.5);
    }

    #[tokio::test]
    async fn complete_explicit_drivers_prepare_without_index_data() {
        let temporary = TempDir::new().unwrap();
        let geospace = Geospace::open(Some(temporary.path())).await.unwrap();
        let query = QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 250.0).unwrap(),
        };
        let prepared = geospace
            .prepare_msis(
                MsisRequest {
                    query,
                    overrides: MsisDriverOverrides {
                        f107a: Some(80.0),
                        f107_previous_day: Some(75.0),
                        geomagnetic_activity: Some(MsisGeomagneticActivity::Daily(5.0)),
                    },
                },
                DataPolicy::Offline,
            )
            .await
            .unwrap();
        assert!(prepared.indices.f107a.is_none());
        assert!(prepared.indices.f107_previous_day.is_none());
        assert!(prepared.indices.ap_daily.is_none());
        assert_eq!(prepared.indices.ap_three_hourly.len(), 0);
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }
}
