//! Strict keyword parsing only; scientific validation stays in the Rust models.
use ionoray_geospace::DataPolicy;
#[cfg(feature = "iri")]
use ionoray_geospace::IriDriverOverrides;
#[cfg(feature = "hwm")]
use ionoray_geospace::hwm::HwmGeomagneticActivity;
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
use ionoray_geospace::{Epoch, GeodeticPosition, QueryPoint};
#[cfg(feature = "msis")]
use ionoray_geospace::{
    MsisDriverOverrides,
    msis::{MsisApHistory, MsisGeomagneticActivity},
};
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
use pyo3::{
    exceptions::PyTypeError,
    types::{PyBool, PyDict},
};
use pyo3::{exceptions::PyValueError, prelude::*};

#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
pub(crate) const POINT: &[&str] = &["at", "latitude_deg", "longitude_deg", "altitude_km"];
#[cfg(feature = "iri")]
pub(crate) const IRI: &[&str] = &["rz12", "ig12", "f107_daily", "f107_81_day"];
#[cfg(feature = "hwm")]
pub(crate) const HWM: &[&str] = &["activity", "current_ap"];
#[cfg(feature = "msis")]
pub(crate) const MSIS: &[&str] = &["f107a", "f107_previous_day", "ap_daily", "ap_history"];

#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
pub(crate) struct Arguments<'py>(Bound<'py, PyDict>);
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
impl<'py> Arguments<'py> {
    pub(crate) fn new(
        py: Python<'py>,
        kwargs: Option<&Bound<'py, PyDict>>,
        groups: &[&[&str]],
    ) -> PyResult<Self> {
        let dict = kwargs.cloned().unwrap_or_else(|| PyDict::new(py));
        for (key, _) in dict.iter() {
            let key = key.extract::<String>()?;
            if !groups.iter().any(|names| names.contains(&key.as_str())) {
                return Err(PyTypeError::new_err(format!("unknown parameter '{key}'")));
            }
        }
        Ok(Self(dict))
    }
    pub(crate) fn required(&self, name: &str) -> PyResult<Bound<'py, PyAny>> {
        self.0
            .get_item(name)?
            .ok_or_else(|| PyTypeError::new_err(format!("missing required parameter '{name}'")))
    }
    #[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
    pub(crate) fn optional(&self, name: &str) -> PyResult<Option<Bound<'py, PyAny>>> {
        Ok(self.0.get_item(name)?.filter(|v| !v.is_none()))
    }
    pub(crate) fn number(&self, name: &str) -> PyResult<f64> {
        number(&self.required(name)?, name)
    }
    #[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
    fn optional_number(&self, name: &str) -> PyResult<Option<f64>> {
        self.optional(name)?.map(|v| number(&v, name)).transpose()
    }
    pub(crate) fn query(&self) -> PyResult<QueryPoint> {
        let at = self.required("at")?.extract::<String>()?;
        let at = at.trim();
        let normalized =
            if let Some(body) = at.strip_suffix('Z').or_else(|| at.strip_suffix("+00:00")) {
                format!("{body} UTC")
            } else if at.ends_with(" UTC") {
                at.to_owned()
            } else {
                return Err(PyValueError::new_err(
                    "at must explicitly specify UTC (Z, +00:00, or UTC)",
                ));
            };
        let epoch = normalized
            .parse::<Epoch>()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let position = GeodeticPosition::from_degrees_kilometers(
            self.number("latitude_deg")?,
            self.number("longitude_deg")?,
            self.number("altitude_km")?,
        )
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(QueryPoint { epoch, position })
    }
    #[cfg(feature = "iri")]
    pub(crate) fn iri(&self, required: bool) -> PyResult<IriDriverOverrides> {
        if required {
            for name in IRI {
                self.number(name)?;
            }
        }
        Ok(IriDriverOverrides {
            rz12: self.optional_number("rz12")?,
            ig12: self.optional_number("ig12")?,
            f107_daily: self.optional_number("f107_daily")?,
            f107_81_day: self.optional_number("f107_81_day")?,
        })
    }
    #[cfg(feature = "hwm")]
    pub(crate) fn hwm(&self, required: bool) -> PyResult<Option<HwmGeomagneticActivity>> {
        if required {
            self.required("activity")?;
        }
        let ap = self.optional_number("current_ap")?;
        let activity = self
            .optional("activity")?
            .map(|v| v.extract::<String>())
            .transpose()?;
        match (activity.as_deref(), ap) {
            (None, None) if !required => Ok(None),
            (Some("quiet"), None) => Ok(Some(HwmGeomagneticActivity::Quiet)),
            (Some("disturbed"), Some(current_ap)) => {
                Ok(Some(HwmGeomagneticActivity::Disturbed { current_ap }))
            }
            _ => Err(PyValueError::new_err(
                "activity must be quiet without current_ap, or disturbed with current_ap",
            )),
        }
    }
    #[cfg(feature = "msis")]
    pub(crate) fn msis(&self, required: bool) -> PyResult<MsisDriverOverrides> {
        if required {
            self.number("f107a")?;
            self.number("f107_previous_day")?;
        }
        let daily = self.optional_number("ap_daily")?;
        let history = self.optional("ap_history")?;
        let geomagnetic_activity = match (daily, history) {
            (Some(ap), None) => Some(MsisGeomagneticActivity::Daily(ap)),
            (None, Some(history)) => {
                Some(MsisGeomagneticActivity::StormTime(parse_history(&history)?))
            }
            (None, None) if !required => None,
            _ => {
                return Err(PyValueError::new_err(
                    "supply exactly one of ap_daily or complete ap_history",
                ));
            }
        };
        Ok(MsisDriverOverrides {
            f107a: self.optional_number("f107a")?,
            f107_previous_day: self.optional_number("f107_previous_day")?,
            geomagnetic_activity,
        })
    }
    #[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
    pub(crate) fn policy(&self, fallback: DataPolicy) -> PyResult<DataPolicy> {
        self.optional("data_policy")?
            .map_or(Ok(fallback), |v| parse_policy(&v.extract::<String>()?))
    }
}
#[cfg(feature = "msis")]
fn parse_history(value: &Bound<'_, PyAny>) -> PyResult<MsisApHistory> {
    let dict = value
        .cast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("ap_history must be a dict"))?;
    let names = &[
        "daily",
        "current",
        "three_hours_ago",
        "six_hours_ago",
        "nine_hours_ago",
        "average_12_to_33_hours",
        "average_36_to_57_hours",
    ];
    let a = Arguments::new(value.py(), Some(dict), &[names])?;
    Ok(MsisApHistory {
        daily: a.number("daily")?,
        current: a.number("current")?,
        three_hours_ago: a.number("three_hours_ago")?,
        six_hours_ago: a.number("six_hours_ago")?,
        nine_hours_ago: a.number("nine_hours_ago")?,
        average_12_to_33_hours: a.number("average_12_to_33_hours")?,
        average_36_to_57_hours: a.number("average_36_to_57_hours")?,
    })
}
pub(crate) fn parse_policy(value: &str) -> PyResult<DataPolicy> {
    match value {
        "ensure" => Ok(DataPolicy::Ensure),
        "offline" => Ok(DataPolicy::Offline),
        "refresh" => Ok(DataPolicy::Refresh),
        _ => Err(PyValueError::new_err(
            "data_policy must be ensure, offline, or refresh",
        )),
    }
}
#[cfg(any(feature = "igrf", feature = "iri", feature = "hwm", feature = "msis"))]
fn number(value: &Bound<'_, PyAny>, name: &str) -> PyResult<f64> {
    if value.is_instance_of::<PyBool>() {
        return Err(PyTypeError::new_err(format!(
            "{name} must be a number, not bool"
        )));
    }
    let value = value.extract::<f64>()?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(PyValueError::new_err(format!("{name} must be finite")))
    }
}
