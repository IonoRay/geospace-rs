//! Stable exception codes derived from typed errors, never message matching.
use ionoray_geospace::{
    GeospaceError as Error, hwm::HwmError, igrf::IgrfError, iri::IriError, msis::MsisError,
};
use ionoray_indices::{IndexError, SourceCheckStatus};
use pyo3::{
    create_exception,
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
};
create_exception!(
    ionoray_geospace,
    GeospaceError,
    PyRuntimeError,
    "Data or model execution failed; inspect code and message."
);

pub(crate) fn failure(py: Python<'_>, code: &str, message: impl std::fmt::Display) -> PyErr {
    let message = message.to_string();
    let error = GeospaceError::new_err(message.clone());
    // Fresh built-in exception instances always have a writable attribute dictionary.
    if let Err(attribute_error) = error
        .value(py)
        .setattr("code", code)
        .and_then(|()| error.value(py).setattr("message", message))
    {
        return attribute_error;
    }
    error
}
pub(crate) fn translate(py: Python<'_>, error: Error) -> PyErr {
    // Geospace may gain feature-gated error variants (for example CLI JSON).
    #[allow(clippy::match_wildcard_for_single_variants)]
    let code = match &error {
        Error::Position(_)
        | Error::InvalidEpoch(_)
        | Error::InvalidYear(_)
        | Error::Igrf(IgrfError::EpochOutOfRange(_) | IgrfError::AltitudeOutOfRange(_))
        | Error::Iri(
            IriError::InvalidDriver { .. }
            | IriError::AltitudeOutOfRange(_)
            | IriError::EpochOutOfRange(_)
            | IriError::InputOutOfRange { .. },
        )
        | Error::Hwm(
            HwmError::InvalidAp(_)
            | HwmError::AltitudeBelowSurface(_)
            | HwmError::InputOutOfRange { .. },
        )
        | Error::Msis(MsisError::InvalidDriver { .. } | MsisError::AltitudeBelowSurface(_)) => {
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
        Error::Iri(IriError::BackendUnavailable)
        | Error::Hwm(HwmError::BackendUnavailable)
        | Error::Msis(MsisError::BackendUnavailable) => "model_unavailable",
        Error::Igrf(_) | Error::Iri(_) | Error::Hwm(_) | Error::Msis(_) => "model_failed",
        _ => "internal_error",
    };
    failure(py, code, error)
}
