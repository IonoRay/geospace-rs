//! Direct synchronous model calls. No runtime or index home is opened.
use crate::{
    convert::output,
    error::translate,
    request::{Arguments, HWM, IRI, MSIS, POINT},
    tracing::PyTracingGuard,
};
use ionoray_geospace::{
    hwm::{Hwm, HwmInput, HwmVersion},
    igrf::{Igrf, IgrfInput, IgrfVersion},
    iri::{Iri, IriDrivers, IriInput, IriVersion},
    msis::{Msis, MsisDrivers, MsisInput, MsisVersion},
};
use pyo3::{prelude::*, types::PyDict};

/// IGRF-14 at an explicit UTC/WGS84 point; returns magnetic field in SI.
#[pyfunction]
#[pyo3(signature = (**kwargs), text_signature = "(*, at, latitude_deg, longitude_deg, altitude_km)")]
pub(crate) fn igrf(py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let a = Arguments::new(py, kwargs, &[POINT])?;
    let input = IgrfInput { query: a.query()? };
    let result = py
        .detach(|| Igrf::new(IgrfVersion::Igrf14)?.evaluate(&input))
        .map_err(|e| translate(py, e.into()))?;
    output(py, &result)
}
/// IRI-2020 with four explicit empirical drivers (F10.7 in sfu).
#[pyfunction]
#[pyo3(signature = (**kwargs), text_signature = "(*, at, latitude_deg, longitude_deg, altitude_km, rz12, ig12, f107_daily, f107_81_day)")]
pub(crate) fn iri(py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let a = Arguments::new(py, kwargs, &[POINT, IRI])?;
    let input = IriInput {
        query: a.query()?,
        drivers: IriDrivers {
            sunspot_number_12_month: a.number("rz12")?,
            ionospheric_index_12_month: a.number("ig12")?,
            f107_daily: a.number("f107_daily")?,
            f107_81_day: a.number("f107_81_day")?,
        },
    };
    let result = py
        .detach(|| Iri::new(IriVersion::Iri2020).evaluate(&input))
        .map_err(|e| translate(py, e.into()))?;
    output(py, &result)
}
/// HWM14 quiet winds or disturbed winds with explicit current ap.
#[pyfunction]
#[pyo3(signature = (**kwargs), text_signature = "(*, at, latitude_deg, longitude_deg, altitude_km, activity, current_ap=None)")]
pub(crate) fn hwm(py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let a = Arguments::new(py, kwargs, &[POINT, HWM])?;
    let input = HwmInput {
        query: a.query()?,
        geomagnetic_activity: a.hwm(true)?.expect("required activity"),
    };
    let result = py
        .detach(|| Hwm::new(HwmVersion::Hwm14).evaluate(&input))
        .map_err(|e| translate(py, e.into()))?;
    output(py, &result)
}
/// NRLMSIS 2.1 with explicit flux and Daily or complete `StormTime` activity.
#[pyfunction]
#[pyo3(signature = (**kwargs), text_signature = "(*, at, latitude_deg, longitude_deg, altitude_km, f107a, f107_previous_day, ap_daily=None, ap_history=None)")]
pub(crate) fn msis(py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
    let a = Arguments::new(py, kwargs, &[POINT, MSIS])?;
    let drivers = a.msis(true)?;
    let input = MsisInput {
        query: a.query()?,
        drivers: MsisDrivers {
            f107a: drivers.f107a.expect("required flux"),
            f107_previous_day: drivers.f107_previous_day.expect("required flux"),
            geomagnetic_activity: drivers.geomagnetic_activity.expect("required activity"),
        },
    };
    let result = py
        .detach(|| Msis::new(MsisVersion::Nrlmsis21).evaluate(&input))
        .map_err(|e| translate(py, e.into()))?;
    output(py, &result)
}
/// Lists this extension's supported public model capabilities.
#[pyfunction]
pub(crate) fn capabilities() -> Vec<&'static str> {
    vec!["igrf", "iri", "hwm", "msis", "indices"]
}
/// Installs process-wide tracing and returns its explicit lifecycle guard.
#[pyfunction]
pub(crate) fn init_tracing() -> PyResult<PyTracingGuard> {
    PyTracingGuard::new().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
}
