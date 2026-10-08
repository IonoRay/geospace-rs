use crate::{api, error::GeospaceError, session::Session};
use pyo3::prelude::*;
/// Native implementation module, re-exported by the Python package.
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Session>()?;
    #[cfg(feature = "iri")]
    module.add_class::<PyPreparedIri>()?;
    #[cfg(feature = "hwm")]
    module.add_class::<PyPreparedHwm>()?;
    #[cfg(feature = "msis")]
    module.add_class::<PyPreparedMsis>()?;
    module.add("GeospaceError", module.py().get_type::<GeospaceError>())?;
    module.add_function(wrap_pyfunction!(api::igrf, module)?)?;
    module.add_function(wrap_pyfunction!(api::iri, module)?)?;
    module.add_function(wrap_pyfunction!(api::hwm, module)?)?;
    module.add_function(wrap_pyfunction!(api::msis, module)?)?;
    module.add_function(wrap_pyfunction!(api::capabilities, module)?)?;
    module.add_function(wrap_pyfunction!(api::init_tracing, module)?)?;
    Ok(())
}

#[cfg(feature = "iri")]
use crate::prepared::PyPreparedIri;

#[cfg(feature = "hwm")]
use crate::prepared::PyPreparedHwm;

#[cfg(feature = "msis")]
use crate::prepared::PyPreparedMsis;
