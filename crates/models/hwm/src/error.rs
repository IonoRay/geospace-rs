use thiserror::Error;

/// HWM construction, initialization, or evaluation failure.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum HwmError {
    /// The official backend was not compiled into this crate.
    #[error("HWM14 backend is disabled; enable the `hwm14` feature")]
    BackendUnavailable,
    /// The current three-hour ap driver was invalid.
    #[error("current three-hour ap must be finite and non-negative, found {0}")]
    InvalidAp(f64),
    /// HWM14 models neutral winds from the surface upward.
    #[error("geodetic altitude {0} km is below the supported surface boundary")]
    AltitudeBelowSurface(f64),
    /// A scalar cannot be represented by the official single-precision ABI.
    #[error("{name} value {value} is outside the HWM14 single-precision range")]
    InputOutOfRange {
        /// Input field name.
        name: &'static str,
        /// Rejected scalar value.
        value: f64,
    },
    /// Build-embedded model assets could not be materialized safely.
    #[error("cannot prepare HWM14 assets: {0}")]
    Asset(String),
    /// The global Fortran backend lock was poisoned by an earlier panic.
    #[error("HWM14 backend state lock is poisoned")]
    BackendPoisoned,
    /// The stable C ABI initialization routine reported a failure.
    #[error("HWM14 initialization failed with status {0}")]
    Initialization(i32),
}
