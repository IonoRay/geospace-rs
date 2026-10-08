//! One explicitly opened store/runtime, confined to its creating Python thread.
use crate::{
    convert::output,
    error::{failure, translate},
    prepared::{PyPreparedHwm, PyPreparedIri, PyPreparedMsis},
    request::{Arguments, HWM, IRI, MSIS, POINT, parse_policy},
};
use ionoray_geospace::{DataPolicy, Geospace, HwmRequest, IriRequest, MsisRequest};
use pyo3::{exceptions::PyRuntimeError, prelude::*, types::PyDict};
use std::{
    path::PathBuf,
    thread::{self, ThreadId},
};
use tokio::runtime::{Builder, Runtime};

// Fields drop in declaration order: release the store before the runtime.
struct Resources {
    geospace: Geospace,
    runtime: Runtime,
}
/// Automatic preparation using one reusable store. Explicit close is recommended.
#[pyclass(module = "ionoray_geospace")]
pub(crate) struct Session {
    owner: ThreadId,
    policy: DataPolicy,
    resources: Option<Resources>,
}
impl Session {
    fn check_thread(&self) -> PyResult<()> {
        if self.owner == thread::current().id() {
            Ok(())
        } else {
            Err(PyRuntimeError::new_err(
                "Session belongs to another Python thread",
            ))
        }
    }
    fn resources(&self) -> PyResult<&Resources> {
        self.check_thread()?;
        self.resources
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("Session is closed"))
    }
}
#[pymethods]
impl Session {
    #[new]
    #[pyo3(signature = (*, home=None, data_policy="ensure"))]
    // PyO3 extracts an owned path from str/os.PathLike before detaching.
    #[allow(clippy::needless_pass_by_value)]
    fn new(py: Python<'_>, home: Option<PathBuf>, data_policy: &str) -> PyResult<Self> {
        let policy = parse_policy(data_policy)?;
        let resources = py
            .detach(|| {
                let runtime = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| ("internal_error", e.to_string()))?;
                let geospace = runtime
                    .block_on(Geospace::open(home.as_deref()))
                    .map_err(|e| ("data_access", e.to_string()))?;
                Ok::<_, (&str, String)>(Resources { geospace, runtime })
            })
            .map_err(|(code, message)| failure(py, code, message))?;
        Ok(Self {
            owner: thread::current().id(),
            policy,
            resources: Some(resources),
        })
    }
    /// Idempotently release the store, then shut down the runtime without the GIL.
    fn close(&mut self, py: Python<'_>) -> PyResult<()> {
        self.check_thread()?;
        let resources = self.resources.take();
        py.detach(|| drop(resources));
        Ok(())
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyResult<PyRef<'_, Self>> {
        slf.resources()?;
        Ok(slf)
    }
    fn __exit__(
        &mut self,
        py: Python<'_>,
        _type: Py<PyAny>,
        _value: Py<PyAny>,
        _traceback: Py<PyAny>,
    ) -> PyResult<()> {
        self.close(py)
    }

    /// Resolve missing Iri drivers once; returned Prepared outlives this session.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, rz12=None, ig12=None, f107_daily=None, f107_81_day=None, data_policy=None)")]
    fn prepare_iri(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyPreparedIri> {
        let resources = self.resources()?;
        let a = Arguments::new(py, kwargs, &[POINT, IRI, &["data_policy"]])?;
        let request = IriRequest {
            query: a.query()?,
            overrides: a.iri(false)?,
        };
        let policy = a.policy(self.policy)?;
        let prepared = py
            .detach(|| {
                resources
                    .runtime
                    .block_on(resources.geospace.prepare_iri(request, policy))
            })
            .map_err(|e| translate(py, e))?;
        Ok(PyPreparedIri(prepared))
    }
    /// Prepare and evaluate using the same Rust implementation as Prepared.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, rz12=None, ig12=None, f107_daily=None, f107_81_day=None, data_policy=None)")]
    fn evaluate_iri(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let prepared = self.prepare_iri(py, kwargs)?;
        let result = py
            .detach(|| prepared.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }

    /// Resolve missing Hwm drivers once; returned Prepared outlives this session.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, activity=None, current_ap=None, data_policy=None)")]
    fn prepare_hwm(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyPreparedHwm> {
        let resources = self.resources()?;
        let a = Arguments::new(py, kwargs, &[POINT, HWM, &["data_policy"]])?;
        let request = HwmRequest {
            query: a.query()?,
            geomagnetic_activity: a.hwm(false)?,
        };
        let policy = a.policy(self.policy)?;
        let prepared = py
            .detach(|| {
                resources
                    .runtime
                    .block_on(resources.geospace.prepare_hwm(request, policy))
            })
            .map_err(|e| translate(py, e))?;
        Ok(PyPreparedHwm(prepared))
    }
    /// Prepare and evaluate using the same Rust implementation as Prepared.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, activity=None, current_ap=None, data_policy=None)")]
    fn evaluate_hwm(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let prepared = self.prepare_hwm(py, kwargs)?;
        let result = py
            .detach(|| prepared.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }

    /// Resolve missing Msis drivers once; returned Prepared outlives this session.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, f107a=None, f107_previous_day=None, ap_daily=None, ap_history=None, data_policy=None)")]
    fn prepare_msis(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyPreparedMsis> {
        let resources = self.resources()?;
        let a = Arguments::new(py, kwargs, &[POINT, MSIS, &["data_policy"]])?;
        let request = MsisRequest {
            query: a.query()?,
            overrides: a.msis(false)?,
        };
        let policy = a.policy(self.policy)?;
        let prepared = py
            .detach(|| {
                resources
                    .runtime
                    .block_on(resources.geospace.prepare_msis(request, policy))
            })
            .map_err(|e| translate(py, e))?;
        Ok(PyPreparedMsis(prepared))
    }
    /// Prepare and evaluate using the same Rust implementation as Prepared.
    #[pyo3(signature = (**kwargs), text_signature = "($self, *, at, latitude_deg, longitude_deg, altitude_km, f107a=None, f107_previous_day=None, ap_daily=None, ap_history=None, data_policy=None)")]
    fn evaluate_msis(
        &self,
        py: Python<'_>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let prepared = self.prepare_msis(py, kwargs)?;
        let result = py
            .detach(|| prepared.0.evaluate())
            .map_err(|e| translate(py, e))?;
        output(py, &result)
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
