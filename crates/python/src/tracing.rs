//! Python ownership adapter for the process-wide tracing guard.

use std::sync::Mutex;

use pyo3::{exceptions::PyRuntimeError, prelude::*};

/// Explicit owner for process-wide tracing resources.
#[pyclass(weakref)]
pub(crate) struct PyTracingGuard {
    guard: Mutex<ionoray_observability::TracingGuard>,
}

impl PyTracingGuard {
    pub(crate) fn new() -> Result<Self, ionoray_observability::TracingInitError> {
        Ok(Self {
            guard: Mutex::new(ionoray_observability::init_tracing()?),
        })
    }
}

#[pymethods]
impl PyTracingGuard {
    /// Flushes logs and releases tracing resources. Repeated calls are harmless.
    fn close(&self) -> PyResult<()> {
        self.guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(&self, _type: Py<PyAny>, _value: Py<PyAny>, _traceback: Py<PyAny>) -> PyResult<()> {
        self.close()
    }
}
