//! Recursive conversion of serialized Rust reports to Python basic values.

use pyo3::{
    exceptions::PyTypeError,
    prelude::*,
    types::{PyBool, PyDict, PyList},
};
use serde_json::Value;

/// Converts JSON-compatible report data without a JSON text round trip.
pub(crate) fn value_to_python(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(value) => Ok(PyBool::new(py, *value).to_owned().into_any().unbind()),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.into_pyobject(py)?.unbind().into_any())
            } else if let Some(value) = value.as_u64() {
                Ok(value.into_pyobject(py)?.unbind().into_any())
            } else if let Some(value) = value.as_f64() {
                Ok(value.into_pyobject(py)?.unbind().into_any())
            } else {
                Err(PyTypeError::new_err(
                    "report contains an unsupported JSON number",
                ))
            }
        }
        Value::String(value) => Ok(value.into_pyobject(py)?.unbind().into_any()),
        Value::Array(values) => {
            let list = PyList::empty(py);
            for value in values {
                list.append(value_to_python(py, value)?)?;
            }
            Ok(list.into_any().unbind())
        }
        Value::Object(values) => {
            let dict = PyDict::new(py);
            for (key, value) in values {
                dict.set_item(key, value_to_python(py, value)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Serializes existing Rust public structures, retaining SI units and null values.
pub(crate) fn output(py: Python<'_>, value: &impl serde::Serialize) -> PyResult<Py<PyAny>> {
    let value =
        serde_json::to_value(value).map_err(|e| crate::error::failure(py, "internal_error", e))?;
    value_to_python(py, &value)
}
