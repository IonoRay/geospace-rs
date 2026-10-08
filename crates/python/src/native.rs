use crate::{
    api,
    error::GeospaceError,
    prepared::{PyPreparedHwm, PyPreparedIri, PyPreparedMsis},
    session::Session,
};
use pyo3::prelude::*;
/// Native implementation module, re-exported by the Python package.
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Session>()?;
    module.add_class::<PyPreparedIri>()?;
    module.add_class::<PyPreparedHwm>()?;
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
