use thiserror::Error;

/// IRI construction, initialization, or evaluation failure.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum IriError {
    /// The official backend was not compiled into this crate.
    #[error("IRI-2020 backend is disabled; enable the `iri2020` feature")]
    BackendUnavailable,
    /// A caller-supplied empirical driver was invalid.
    #[error("{name} must be {constraint}, found {value}")]
    InvalidDriver {
        /// Driver field name.
        name: &'static str,
        /// Domain constraint violated by the value.
        constraint: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// The point API is restricted to the common IRI plasma altitude domain.
    #[error("geodetic altitude {0} km is outside the supported 60..=1500 km interval")]
    AltitudeOutOfRange(f64),
    /// The epoch cannot be evaluated by the pinned IRI/IGRF asset set.
    #[error("UTC year {0} is outside the supported 1958..=2030 interval")]
    EpochOutOfRange(i32),
    /// A scalar cannot be represented by the official single-precision ABI.
    #[error("{name} value {value} is outside the IRI-2020 single-precision range")]
    InputOutOfRange {
        /// Input field name.
        name: &'static str,
        /// Rejected scalar value.
        value: f64,
    },
    /// Build-embedded model assets could not be materialized safely.
    #[error("cannot prepare IRI-2020 assets: {0}")]
    Asset(String),
    /// The global Fortran backend lock was poisoned by an earlier panic.
    #[error("IRI-2020 backend state lock is poisoned")]
    BackendPoisoned,
    /// The stable C ABI initialization routine reported a failure.
    #[error("IRI-2020 initialization failed with status {0}")]
    Initialization(i32),
    /// The stable C ABI evaluation routine reported a failure.
    #[error("IRI-2020 evaluation failed with status {0}")]
    Evaluation(i32),
    /// The official backend returned a non-finite result.
    #[error("IRI-2020 returned a non-finite {0}")]
    NonFiniteOutput(&'static str),
}
