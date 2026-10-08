use thiserror::Error;

/// IGRF construction or evaluation failure.
#[derive(Clone, Debug, Error, PartialEq)]
pub enum IgrfError {
    /// The embedded bytes differ from the pinned official coefficient asset.
    #[error("IGRF coefficient SHA-256 mismatch: expected {expected}, got {actual}")]
    CoefficientDigestMismatch {
        /// Pinned official SHA-256.
        expected: String,
        /// SHA-256 of the embedded bytes.
        actual: String,
    },
    /// An embedded coefficient row was malformed.
    #[error("invalid embedded IGRF coefficient at line {line}: {reason}")]
    InvalidCoefficient {
        /// One-based source line.
        line: usize,
        /// Validation failure.
        reason: String,
    },
    /// The embedded table did not contain every required coefficient.
    #[error("embedded IGRF coefficient table is incomplete: parsed {parsed}, expected {expected}")]
    IncompleteCoefficients {
        /// Parsed coefficient rows.
        parsed: usize,
        /// Required coefficient rows.
        expected: usize,
    },
    /// The requested epoch is outside the published model interval.
    #[error("decimal year {0:.9} is outside the supported IGRF-14 interval [1900, 2030]")]
    EpochOutOfRange(f64),
    /// Altitude would place the query at or below Earth's center.
    #[error("geodetic altitude {0} km is outside the supported range")]
    AltitudeOutOfRange(f64),
}
