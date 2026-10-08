use ionoray_core::{Duration, QueryPoint};
use ionoray_hwm::{Hwm, HwmGeomagneticActivity, HwmInput, HwmResult, HwmVersion};
use ionoray_indices::{Ap, IndexDataset, IndexField, IndexSample};
use serde::{Deserialize, Serialize};

use crate::{DataPolicy, Geospace, GeospaceError};

/// High-level HWM request with optional explicit environmental input.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HwmRequest {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Explicit activity mode; `None` resolves the current three-hour ap index.
    pub geomagnetic_activity: Option<HwmGeomagneticActivity>,
}

impl HwmRequest {
    /// Creates a request whose environmental driver is resolved from indices.
    pub const fn new(query: QueryPoint) -> Self {
        Self {
            query,
            geomagnetic_activity: None,
        }
    }
}

/// Reusable HWM input prepared from explicit or indexed drivers.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedHwm {
    /// Exact low-level model input that will be evaluated.
    input: HwmInput,
    /// GFZ sample used for automatic ap resolution; absent for explicit input.
    ap_index: Option<IndexSample<Ap>>,
}

/// HWM output paired with its exact resolved input and index evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HwmEvaluation {
    /// Exact input evaluated by HWM14.
    pub input: HwmInput,
    /// GFZ sample used for automatic ap resolution; absent for explicit input.
    pub ap_index: Option<IndexSample<Ap>>,
    /// Scientific model result and provenance.
    pub result: HwmResult,
}

impl Geospace {
    /// Resolves HWM input under an explicit data-access policy.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when required index data cannot be prepared or read.
    pub async fn prepare_hwm(
        &self,
        request: HwmRequest,
        policy: DataPolicy,
    ) -> Result<PreparedHwm, GeospaceError> {
        let mut stage = crate::runner::StageTrace::new("model.prepare", "hwm14");
        let span = stage.span();
        tracing::Instrument::instrument(
            async {
                if let Some(geomagnetic_activity) = request.geomagnetic_activity {
                    let prepared = PreparedHwm {
                        input: HwmInput {
                            query: request.query,
                            geomagnetic_activity,
                        },
                        ap_index: None,
                    };
                    stage.succeeded(0, "explicit");
                    return Ok(prepared);
                }

                self.prepare_fields_range(
                    IndexDataset::KpApF107,
                    vec![IndexField::Ap3h],
                    request.query.epoch,
                    request.query.epoch + Duration::from_milliseconds(1.0),
                    policy,
                )
                .await?;
                let ap_index = self.indices().ap_at(request.query.epoch).await?;
                let geomagnetic_activity = HwmGeomagneticActivity::Disturbed {
                    current_ap: ap_index.value.value(),
                };
                let prepared = PreparedHwm {
                    input: HwmInput {
                        query: request.query,
                        geomagnetic_activity,
                    },
                    ap_index: Some(ap_index),
                };
                stage.succeeded(1, "indices");
                Ok(prepared)
            },
            span,
        )
        .await
    }

    /// Resolves missing HWM drivers and evaluates HWM14.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] for data preparation or model evaluation failures.
    pub async fn evaluate_hwm(
        &self,
        request: HwmRequest,
        policy: DataPolicy,
    ) -> Result<HwmEvaluation, GeospaceError> {
        self.prepare_hwm(request, policy).await?.evaluate()
    }
}

impl PreparedHwm {
    /// Copies this baseline with explicit activity and clears the ap evidence.
    ///
    /// This also clears evidence when the activity is unchanged. No indices are
    /// read; validation occurs at evaluation.
    #[must_use]
    pub fn with_activity(&self, activity: HwmGeomagneticActivity) -> Self {
        let mut scenario = self.clone();
        scenario.input.geomagnetic_activity = activity;
        scenario.ap_index = None;
        scenario
    }

    /// Returns the exact resolved model input.
    pub const fn input(&self) -> &HwmInput {
        &self.input
    }

    /// Returns the evidence for automatically resolved drivers.
    pub fn ap_index(&self) -> Option<&IndexSample<Ap>> {
        self.ap_index.as_ref()
    }

    /// Executes the resolved HWM input without store or network access.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when HWM initialization or evaluation fails.
    pub fn evaluate(&self) -> Result<HwmEvaluation, GeospaceError> {
        let result = Hwm::new(HwmVersion::Hwm14).evaluate(&self.input)?;
        Ok(HwmEvaluation {
            input: self.input,
            ap_index: self.ap_index.clone(),
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
    async fn explicit_activity_prepares_without_index_data() {
        let temporary = TempDir::new().unwrap();
        let geospace = Geospace::open(Some(temporary.path())).await.unwrap();
        let query = QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 250.0).unwrap(),
        };
        let prepared = geospace
            .prepare_hwm(
                HwmRequest {
                    query,
                    geomagnetic_activity: Some(HwmGeomagneticActivity::Quiet),
                },
                DataPolicy::Offline,
            )
            .await
            .unwrap();
        assert_eq!(
            prepared.input.geomagnetic_activity,
            HwmGeomagneticActivity::Quiet
        );
        assert!(prepared.ap_index.is_none());
    }

    #[tokio::test]
    async fn offline_preparation_resolves_the_requested_three_hour_sample() {
        let temporary = TempDir::new().unwrap();
        let geospace = Geospace::open(Some(temporary.path())).await.unwrap();
        let epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 1, 13, 17, 0, 0).unwrap();
        let query = QueryPoint {
            epoch,
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 250.0).unwrap(),
        };
        let prepared = geospace
            .prepare_hwm(HwmRequest::new(query), DataPolicy::Offline)
            .await
            .unwrap();
        let sample = prepared.ap_index.unwrap();
        assert!(sample.interval.start <= epoch && epoch < sample.interval.end);
        assert_ne!(sample.release_id.len(), 0);
        assert_eq!(
            prepared.input.geomagnetic_activity,
            HwmGeomagneticActivity::Disturbed {
                current_ap: sample.value.value()
            }
        );
    }
}
