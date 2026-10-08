use ionoray_core::{Duration, QueryPoint};
use ionoray_indices::{AdjustedF107, Ig12, IndexDataset, IndexField, IndexSample, Rz12};
use ionoray_iri::{Iri, IriDrivers, IriInput, IriResult, IriVersion};
use serde::{Deserialize, Serialize};

use crate::{DataPolicy, Geospace, GeospaceError};

/// Optional IRI drivers: omitted fields are resolved during preparation or inherited in scenarios.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IriDriverOverrides {
    /// Explicit interpolated Rz12.
    pub rz12: Option<f64>,
    /// Explicit interpolated IG12.
    pub ig12: Option<f64>,
    /// Explicit daily adjusted F10.7.
    pub f107_daily: Option<f64>,
    /// Explicit centered 81-day adjusted F10.7.
    pub f107_81_day: Option<f64>,
}

/// High-level IRI request defined by spacetime and optional driver overrides.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriRequest {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Explicit fields that take precedence over index-derived values.
    pub overrides: IriDriverOverrides,
}

impl IriRequest {
    /// Creates a request whose complete empirical input comes from official IRI indices.
    pub const fn new(query: QueryPoint) -> Self {
        Self {
            query,
            overrides: IriDriverOverrides {
                rz12: None,
                ig12: None,
                f107_daily: None,
                f107_81_day: None,
            },
        }
    }
}

/// Exact official index samples consumed while resolving an IRI request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IriIndexEvidence {
    /// Interpolated Rz12 sample, absent when overridden.
    pub rz12: Option<IndexSample<Rz12>>,
    /// Interpolated IG12 sample, absent when overridden.
    pub ig12: Option<IndexSample<Ig12>>,
    /// Daily adjusted F10.7 sample, absent when overridden.
    pub f107_daily: Option<IndexSample<AdjustedF107>>,
    /// Centered 81-day adjusted F10.7 sample, absent when overridden.
    pub f107_81_day: Option<IndexSample<AdjustedF107>>,
}

/// Reusable IRI input prepared from explicit and indexed drivers.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedIri {
    /// Exact low-level model input that will be evaluated.
    input: IriInput,
    /// Index evidence for every automatically resolved driver.
    indices: IriIndexEvidence,
}

/// IRI output paired with its exact resolved input and index evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriEvaluation {
    /// Exact input evaluated by IRI-2020.
    pub input: IriInput,
    /// Index evidence for every automatically resolved driver.
    pub indices: IriIndexEvidence,
    /// Scientific model result and provenance.
    pub result: IriResult,
}

impl Geospace {
    /// Resolves missing IRI drivers under an explicit data-access policy.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when required index data cannot be prepared or read.
    pub async fn prepare_iri(
        &self,
        request: IriRequest,
        policy: DataPolicy,
    ) -> Result<PreparedIri, GeospaceError> {
        let mut stage = crate::runner::StageTrace::new("model.prepare", "iri2020");
        let span = stage.span();
        tracing::Instrument::instrument(
            async {
                let mut evidence = IriIndexEvidence::default();
                for (dataset, fields) in [
                    (
                        IndexDataset::IriIgRz,
                        [
                            (request.overrides.rz12.is_none(), IndexField::Rz12),
                            (request.overrides.ig12.is_none(), IndexField::Ig12),
                        ],
                    ),
                    (
                        IndexDataset::IriApF107,
                        [
                            (request.overrides.f107_daily.is_none(), IndexField::IriF107),
                            (
                                request.overrides.f107_81_day.is_none(),
                                IndexField::IriF107a81,
                            ),
                        ],
                    ),
                ] {
                    let fields: Vec<_> = fields
                        .into_iter()
                        .filter_map(|(needed, field)| needed.then_some(field))
                        .collect();
                    if !fields.is_empty() {
                        self.prepare_fields_range(
                            dataset,
                            fields,
                            request.query.epoch,
                            request.query.epoch + Duration::from_milliseconds(1.0),
                            policy,
                        )
                        .await?;
                    }
                }

                let rz12 = if let Some(value) = request.overrides.rz12 {
                    value
                } else {
                    let sample = self.indices().iri_rz12_at(request.query.epoch).await?;
                    let value = sample.value.value();
                    evidence.rz12 = Some(sample);
                    value
                };
                let ig12 = if let Some(value) = request.overrides.ig12 {
                    value
                } else {
                    let sample = self.indices().iri_ig12_at(request.query.epoch).await?;
                    let value = sample.value.value();
                    evidence.ig12 = Some(sample);
                    value
                };
                let f107_daily = if let Some(value) = request.overrides.f107_daily {
                    value
                } else {
                    let sample = self
                        .indices()
                        .iri_f107_daily_at(request.query.epoch)
                        .await?;
                    let value = sample.value.value();
                    evidence.f107_daily = Some(sample);
                    value
                };
                let f107_81_day = if let Some(value) = request.overrides.f107_81_day {
                    value
                } else {
                    let sample = self
                        .indices()
                        .iri_f107_81_day_at(request.query.epoch)
                        .await?;
                    let value = sample.value.value();
                    evidence.f107_81_day = Some(sample);
                    value
                };

                let prepared = PreparedIri {
                    input: IriInput {
                        query: request.query,
                        drivers: IriDrivers {
                            sunspot_number_12_month: rz12,
                            ionospheric_index_12_month: ig12,
                            f107_daily,
                            f107_81_day,
                        },
                    },
                    indices: evidence,
                };
                Ok(finish_iri_preparation(prepared, &mut stage))
            },
            span,
        )
        .await
    }

    /// Resolves missing IRI drivers and evaluates IRI-2020.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] for data preparation or model evaluation failures.
    pub async fn evaluate_iri(
        &self,
        request: IriRequest,
        policy: DataPolicy,
    ) -> Result<IriEvaluation, GeospaceError> {
        self.prepare_iri(request, policy).await?.evaluate()
    }
}

fn finish_iri_preparation(
    prepared: PreparedIri,
    stage: &mut crate::runner::StageTrace,
) -> PreparedIri {
    let sample_count = usize::from(prepared.indices.rz12.is_some())
        + usize::from(prepared.indices.ig12.is_some())
        + usize::from(prepared.indices.f107_daily.is_some())
        + usize::from(prepared.indices.f107_81_day.is_some());
    stage.succeeded(
        sample_count,
        if sample_count == 0 {
            "explicit"
        } else {
            "indices_or_mixed"
        },
    );
    prepared
}

impl PreparedIri {
    /// Copies this baseline and replaces only explicitly supplied drivers.
    ///
    /// `None` inherits input and evidence. `Some`, even the same value, clears
    /// that field's evidence. No indices are read; validation occurs at evaluation.
    #[must_use]
    pub fn with_overrides(&self, overrides: IriDriverOverrides) -> Self {
        let mut scenario = self.clone();
        if let Some(value) = overrides.rz12 {
            scenario.input.drivers.sunspot_number_12_month = value;
            scenario.indices.rz12 = None;
        }
        if let Some(value) = overrides.ig12 {
            scenario.input.drivers.ionospheric_index_12_month = value;
            scenario.indices.ig12 = None;
        }
        if let Some(value) = overrides.f107_daily {
            scenario.input.drivers.f107_daily = value;
            scenario.indices.f107_daily = None;
        }
        if let Some(value) = overrides.f107_81_day {
            scenario.input.drivers.f107_81_day = value;
            scenario.indices.f107_81_day = None;
        }
        scenario
    }

    /// Returns the exact resolved model input.
    pub const fn input(&self) -> &IriInput {
        &self.input
    }

    /// Returns the evidence for automatically resolved drivers.
    pub const fn indices(&self) -> &IriIndexEvidence {
        &self.indices
    }

    /// Executes the resolved IRI input without store or network access.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when IRI initialization or evaluation fails.
    pub fn evaluate(&self) -> Result<IriEvaluation, GeospaceError> {
        let result = Iri::new(IriVersion::Iri2020).evaluate(&self.input)?;
        Ok(IriEvaluation {
            input: self.input,
            indices: self.indices.clone(),
            result,
        })
    }
}

#[cfg(test)]
mod tests {
    use ionoray_core::{Epoch, GeodeticPosition};
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn complete_explicit_drivers_prepare_offline_without_index_data() {
        let temporary = TempDir::new().unwrap();
        let geospace = Geospace::open(Some(temporary.path())).await.unwrap();
        let query = QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
        };
        let prepared = geospace
            .prepare_iri(
                IriRequest {
                    query,
                    overrides: IriDriverOverrides {
                        rz12: Some(70.0),
                        ig12: Some(80.0),
                        f107_daily: Some(75.0),
                        f107_81_day: Some(76.0),
                    },
                },
                DataPolicy::Offline,
            )
            .await
            .unwrap();
        assert!((prepared.input.drivers.f107_daily - 75.0).abs() < f64::EPSILON);
        assert_eq!(prepared.indices, IriIndexEvidence::default());
    }
}
