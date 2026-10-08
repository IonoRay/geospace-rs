use ionoray_core::{Epoch, Sha256Digest};
use serde::{Deserialize, Serialize};

/// Closed-open validity interval `[start, end)` for one observation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeInterval {
    /// Inclusive start epoch.
    pub start: Epoch,
    /// Exclusive end epoch.
    pub end: Epoch,
}

/// Source quality state without inventing confidence from a numeric value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum QualityFlag {
    /// Source has published the final value.
    Final,
    /// Source identifies the value as provisional.
    Provisional,
    /// The source documents the value as prediction-based.
    Predicted,
    /// No source quality flag was available.
    Unknown,
}

/// Transparent derivation metadata for a returned value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum ValueDerivation {
    /// Value was read directly from the selected source artifact.
    Source,
    /// A missing daily value was bounded by observations and linearly interpolated.
    LinearInterpolation {
        /// Number of consecutive missing days in the interpolated gap.
        gap_days: u16,
    },
    /// Arithmetic mean over a centered daily window.
    CenteredMean {
        /// Number of calendar days in the centered window.
        window_days: u16,
        /// Number of window inputs supplied by linear interpolation.
        interpolated_input_count: u16,
    },
}

/// A typed observation plus immutable source and release identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndexSample<T> {
    /// Domain value.
    pub value: T,
    /// Time interval represented by the sample.
    pub interval: TimeInterval,
    /// Source-provided quality.
    pub quality: QualityFlag,
    /// Whether the value is direct or was derived from source observations.
    pub derivation: ValueDerivation,
    /// Stable release identifier.
    pub release_id: String,
    /// Exact raw source artifact.
    pub artifact: Sha256Digest,
    /// Complete data snapshot selected for the query.
    pub snapshot: Sha256Digest,
}
