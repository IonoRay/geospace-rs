use thiserror::Error;

/// NRLMSIS construction or evaluation failure.
#[derive(Debug, Error)]
pub enum MsisError {
    /// The official backend was not compiled into this crate.
    #[error("NRLMSIS 2.1 backend is disabled; enable the `nrlmsis21` feature")]
    BackendUnavailable,
    /// A scalar model driver was invalid.
    #[error("{name} must be finite and non-negative, found {value}")]
    InvalidDriver {
        /// Driver field name.
        name: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// NRLMSIS models the atmosphere from the surface upward.
    #[error("geodetic altitude {0} km is below the supported surface boundary")]
    AltitudeBelowSurface(f64),
    /// A build-embedded parameter asset could not be materialized safely.
    #[error("cannot prepare NRLMSIS 2.1 parameter asset: {0}")]
    Asset(String),
    /// The global Fortran backend lock was poisoned by an earlier panic.
    #[error("NRLMSIS 2.1 backend state lock is poisoned")]
    BackendPoisoned,
    /// The stable C ABI initialization routine reported a failure.
    #[error("NRLMSIS 2.1 initialization failed with status {0}")]
    Initialization(i32),
}
