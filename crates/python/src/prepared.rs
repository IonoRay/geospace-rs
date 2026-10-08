//! Python Prepared objects own Rust inputs/evidence, never a Session.
use crate::{convert::output, error::translate, request::Arguments};

use pyo3::{prelude::*, types::PyDict};

/// Independent, immutable copy of a resolved Iri input and evidence.
#[cfg(feature = "iri")]
#[pyclass(name = "PreparedIri", module = "ionoray_geospace", frozen)]
pub(crate) struct PyPreparedIri(pub(crate) PreparedIri);
#[cfg(feature = "iri")]
#[pymethods]
impl PyPreparedIri {
    #[getter]
    fn input(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, self.0.input())
    }
    #[getter]
    fn indices(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, &self.0.indices())
    }
    /// Recompute through the model without accessing any index store.
    fn evaluate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let result = py
            .detach(|| self.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }
    /// Copy the baseline and mark supplied drivers as explicit.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, rz12=None, ig12=None, f107_daily=None, f107_81_day=None)")]
    fn with_overrides(&self, py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let a = Arguments::new(py, kwargs, &[IRI])?;
        Ok(Self(self.0.with_overrides(a.iri(false)?)))
    }
}

/// Independent, immutable copy of a resolved Hwm input and evidence.
#[cfg(feature = "hwm")]
#[pyclass(name = "PreparedHwm", module = "ionoray_geospace", frozen)]
pub(crate) struct PyPreparedHwm(pub(crate) PreparedHwm);
#[cfg(feature = "hwm")]
#[pymethods]
impl PyPreparedHwm {
    #[getter]
    fn input(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, self.0.input())
    }
    #[getter]
    fn ap_index(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, &self.0.ap_index())
    }
    /// Recompute through the model without accessing any index store.
    fn evaluate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let result = py
            .detach(|| self.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }
    /// Copy the baseline and mark supplied drivers as explicit.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, activity, current_ap=None)")]
    fn with_activity(&self, py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let a = Arguments::new(py, kwargs, &[HWM])?;
        Ok(Self(
            self.0
                .with_activity(a.hwm(true)?.expect("required activity")),
        ))
    }
}

/// Independent, immutable copy of a resolved Msis input and evidence.
#[cfg(feature = "msis")]
#[pyclass(name = "PreparedMsis", module = "ionoray_geospace", frozen)]
pub(crate) struct PyPreparedMsis(pub(crate) PreparedMsis);
#[cfg(feature = "msis")]
#[pymethods]
impl PyPreparedMsis {
    #[getter]
    fn input(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, self.0.input())
    }
    #[getter]
    fn indices(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        output(py, &self.0.indices())
    }
    /// Recompute through the model without accessing any index store.
    fn evaluate(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let result = py
            .detach(|| self.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }
    /// Copy the baseline and mark supplied drivers as explicit.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, f107a=None, f107_previous_day=None, ap_daily=None, ap_history=None)")]
    fn with_overrides(&self, py: Python<'_>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let a = Arguments::new(py, kwargs, &[MSIS])?;
        Ok(Self(self.0.with_overrides(a.msis(false)?)))
    }
}

#[cfg(feature = "iri")]
use crate::request::IRI;
#[cfg(feature = "iri")]
use ionoray_geospace::PreparedIri;

#[cfg(feature = "hwm")]
use crate::request::HWM;
#[cfg(feature = "hwm")]
use ionoray_geospace::PreparedHwm;

#[cfg(feature = "msis")]
use crate::request::MSIS;
#[cfg(feature = "msis")]
use ionoray_geospace::PreparedMsis;
