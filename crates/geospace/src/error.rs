use thiserror::Error;

/// Integrated data preparation or model execution failure.
#[derive(Debug, Error)]
pub enum GeospaceError {
    /// A command-line stream could not be read or written.
    #[cfg(feature = "cli")]
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A command-line model record is structurally invalid.
    #[cfg(feature = "cli")]
    #[error("invalid model request: {0}")]
    CliInput(String),
    /// Maintenance finished but required index coverage remains incomplete.
    #[cfg(feature = "indices")]
    #[error("index maintenance is partial: {}", .0.diagnostic())]
    PartialIndices(Box<ionoray_indices::RangeSyncReport>),
    /// Local data infrastructure failed.
    #[cfg(any(feature = "indices", feature = "cli"))]
    #[error(transparent)]
    Store(#[from] ionoray_store::StoreError),
    /// Geophysical index download or validation failed.
    #[cfg(feature = "indices")]
    #[error(transparent)]
    Indices(#[from] ionoray_indices::IndexError),
    /// A geodetic input was invalid.
    #[error(transparent)]
    Position(#[from] ionoray_core::PositionError),
    /// IGRF evaluation failed.
    #[cfg(feature = "igrf")]
    #[error(transparent)]
    Igrf(#[from] ionoray_igrf::IgrfError),
    /// IRI evaluation failed.
    #[cfg(feature = "iri")]
    #[error(transparent)]
    Iri(#[from] ionoray_iri::IriError),
    /// HWM evaluation failed.
    #[cfg(feature = "hwm")]
    #[error(transparent)]
    Hwm(#[from] ionoray_hwm::HwmError),
    /// CLI result serialization failed.
    #[cfg(feature = "cli")]
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// User supplied an invalid UTC epoch.
    #[error("invalid epoch: {0}")]
    InvalidEpoch(String),
    /// A model query year cannot be represented by the index store.
    #[error("invalid model query year: {0}")]
    InvalidYear(i32),
    /// NRLMSIS input preparation produced an invalid sample collection.
    #[cfg(feature = "msis")]
    #[error("invalid NRLMSIS driver preparation: {0}")]
    InvalidMsisPreparation(&'static str),
    /// NRLMSIS evaluation failed.
    #[cfg(feature = "msis")]
    #[error(transparent)]
    Msis(#[from] ionoray_msis::MsisError),
}
