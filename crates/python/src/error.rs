//! Stable exception codes derived from typed errors, never message matching.
#[cfg(any(
    feature = "igrf",
    feature = "iri",
    feature = "hwm",
    feature = "msis",
    test
))]
use ionoray_geospace::GeospaceError as Error;
#[cfg(feature = "hwm")]
use ionoray_geospace::hwm::HwmError;
#[cfg(feature = "igrf")]
use ionoray_geospace::igrf::IgrfError;
#[cfg(feature = "iri")]
use ionoray_geospace::iri::IriError;
#[cfg(feature = "msis")]
use ionoray_geospace::msis::MsisError;
#[cfg(any(
    feature = "igrf",
    feature = "iri",
    feature = "hwm",
    feature = "msis",
    test
))]
use ionoray_indices::{IndexError, SourceCheckStatus};
use pyo3::{create_exception, exceptions::PyRuntimeError, prelude::*};
create_exception!(
    ionoray_geospace,
    GeospaceError,
    PyRuntimeError,
    "Data or model execution failed; inspect code and message."
);
pub(crate) fn failure(py: Python<'_>, code: &str, message: impl std::fmt::Display) -> PyErr {
    let message = message.to_string();
    let error = GeospaceError::new_err(message.clone());
    if let Err(attribute_error) = error
        .value(py)
        .setattr("code", code)
        .and_then(|()| error.value(py).setattr("message", message))
    {
        return attribute_error;
    }
    error
}
#[cfg(any(
    feature = "igrf",
    feature = "iri",
    feature = "hwm",
    feature = "msis",
    test
))]
pub(crate) fn translate(py: Python<'_>, error: Error) -> PyErr {
    // Workspace feature unification can add CLI-only variants to GeospaceError.
    #[allow(unreachable_patterns, clippy::match_wildcard_for_single_variants)]
    let code = match &error {
        Error::Position(_) | Error::InvalidEpoch(_) | Error::InvalidYear(_) => {
            return PyValueError::new_err(error.to_string());
        }
        #[cfg(feature = "igrf")]
        Error::Igrf(IgrfError::EpochOutOfRange(_) | IgrfError::AltitudeOutOfRange(_)) => {
            return PyValueError::new_err(error.to_string());
        }
        #[cfg(feature = "iri")]
        Error::Iri(
            IriError::InvalidDriver { .. }
            | IriError::AltitudeOutOfRange(_)
            | IriError::EpochOutOfRange(_)
            | IriError::InputOutOfRange { .. },
        ) => return PyValueError::new_err(error.to_string()),
        #[cfg(feature = "hwm")]
        Error::Hwm(
            HwmError::InvalidAp(_)
            | HwmError::AltitudeBelowSurface(_)
            | HwmError::InputOutOfRange { .. },
        ) => return PyValueError::new_err(error.to_string()),
        #[cfg(feature = "msis")]
        Error::Msis(MsisError::InvalidDriver { .. } | MsisError::AltitudeBelowSurface(_)) => {
            return PyValueError::new_err(error.to_string());
        }
        Error::PartialIndices(report)
            if report.source_check_status == SourceCheckStatus::Failed =>
        {
            "data_refresh_failed"
        }
        Error::PartialIndices(_)
        | Error::Indices(
            IndexError::MissingYearData { .. }
            | IndexError::MissingValue { .. }
            | IndexError::UnsupportedYear { .. }
            | IndexError::SourceUnavailable { .. },
        ) => "data_unavailable",
        Error::Store(_) | Error::Indices(_) => "data_access",
        #[cfg(feature = "iri")]
        Error::Iri(IriError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "hwm")]
        Error::Hwm(HwmError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "msis")]
        Error::Msis(MsisError::BackendUnavailable) => "model_unavailable",
        #[cfg(feature = "igrf")]
        Error::Igrf(_) => "model_failed",
        #[cfg(feature = "iri")]
        Error::Iri(_) => "model_failed",
        #[cfg(feature = "hwm")]
        Error::Hwm(_) => "model_failed",
        #[cfg(feature = "msis")]
        Error::Msis(_) => "model_failed",
        #[cfg(feature = "msis")]
        Error::InvalidMsisPreparation(_) => "internal_error",
        _ => "internal_error",
    };
    failure(py, code, error)
}

#[cfg(any(
    feature = "igrf",
    feature = "iri",
    feature = "hwm",
    feature = "msis",
    test
))]
use pyo3::exceptions::PyValueError;
