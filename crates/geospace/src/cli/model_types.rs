use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Model {
    Igrf14,
    Iri2020,
    Hwm14,
    Nrlmsis21,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    Direct,
    Auto,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Policy {
    Ensure,
    Offline,
    Refresh,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Drivers {
    pub(super) rz12: Option<f64>,
    pub(super) ig12: Option<f64>,
    pub(super) f107_daily: Option<f64>,
    pub(super) f107_81_day: Option<f64>,
    pub(super) activity: Option<Activity>,
    pub(super) current_ap: Option<f64>,
    pub(super) f107a: Option<f64>,
    pub(super) f107_previous_day: Option<f64>,
    pub(super) ap_daily: Option<f64>,
    pub(super) ap_history: Option<ApHistory>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Activity {
    Quiet,
    Disturbed,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApHistory {
    pub(super) daily: f64,
    pub(super) current: f64,
    pub(super) three_hours_ago: f64,
    pub(super) six_hours_ago: f64,
    pub(super) nine_hours_ago: f64,
    pub(super) average_12_to_33_hours: f64,
    pub(super) average_36_to_57_hours: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) id: Option<String>,
    pub(super) model: Model,
    pub(super) mode: Mode,
    pub(super) at: String,
    pub(super) latitude_deg: f64,
    pub(super) longitude_deg: f64,
    pub(super) altitude_km: f64,
    pub(super) drivers: Option<Drivers>,
    pub(super) data_policy: Option<Policy>,
}

#[derive(Serialize)]
pub(super) struct ErrorRecord {
    pub(super) code: &'static str,
    pub(super) message: String,
}

#[derive(Serialize)]
pub(super) struct Record {
    pub(super) schema_version: u32,
    pub(super) kind: &'static str,
    pub(super) line: usize,
    pub(super) id: Option<String>,
    pub(super) model: Option<Model>,
    pub(super) status: &'static str,
    pub(super) evaluation: Option<serde_json::Value>,
    pub(super) error: Option<ErrorRecord>,
}

impl Record {
    pub(super) fn failed(
        line: usize,
        id: Option<String>,
        model: Option<Model>,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            kind: "model_evaluation",
            line,
            id,
            model,
            status: "failed",
            evaluation: None,
            error: Some(ErrorRecord {
                code,
                message: message.into(),
            }),
        }
    }
    pub(super) fn succeeded(line: usize, request: &Request, evaluation: serde_json::Value) -> Self {
        Self {
            schema_version: 1,
            kind: "model_evaluation",
            line,
            id: request.id.clone(),
            model: Some(request.model),
            status: "succeeded",
            evaluation: Some(evaluation),
            error: None,
        }
    }
}

impl Drivers {
    pub(super) fn validate_finite(&self) -> Result<(), String> {
        let mut values = vec![
            self.rz12,
            self.ig12,
            self.f107_daily,
            self.f107_81_day,
            self.current_ap,
            self.f107a,
            self.f107_previous_day,
            self.ap_daily,
        ];
        if let Some(h) = self.ap_history {
            values.extend(
                [
                    h.daily,
                    h.current,
                    h.three_hours_ago,
                    h.six_hours_ago,
                    h.nine_hours_ago,
                    h.average_12_to_33_hours,
                    h.average_36_to_57_hours,
                ]
                .map(Some),
            );
        }
        if values.into_iter().flatten().any(|v| !v.is_finite()) {
            return Err("drivers must be finite".into());
        }
        // Validate activity shape even when HWM was not compiled.
        match (self.activity, self.current_ap) {
            (Some(Activity::Quiet) | None, Some(_)) | (Some(Activity::Disturbed), None) => Err(
                "quiet forbids current_ap; disturbed requires it; current_ap requires activity"
                    .into(),
            ),
            _ => Ok(()),
        }
    }
}
