//! Assembly layer for `IonoRay` data products and independent model families.

#[cfg(feature = "cli")]
/// Command-line parsing and execution through the public assembly APIs.
pub mod cli;

#[cfg(feature = "indices")]
mod data_policy;
mod error;
#[cfg(all(feature = "indices", feature = "hwm"))]
mod prepare_hwm;
#[cfg(all(feature = "indices", feature = "iri"))]
mod prepare_iri;
#[cfg(all(feature = "indices", feature = "msis"))]
mod prepare_msis;
#[cfg(feature = "indices")]
mod runner;

#[cfg(feature = "indices")]
pub use data_policy::DataPolicy;
pub use error::GeospaceError;
pub use ionoray_core::{
    Duration, Epoch, GeodeticPosition, PositionError, QueryPoint, TimeScale, units,
};

/// Direct IGRF model API, re-exported without data-store access.
#[cfg(feature = "igrf")]
pub mod igrf {
    pub use ionoray_igrf::{Igrf, IgrfError, IgrfInput, IgrfProvenance, IgrfResult, IgrfVersion};
}

/// Direct IRI model API, re-exported without data-store access.
#[cfg(feature = "iri")]
pub mod iri {
    pub use ionoray_iri::{
        Iri, IriDrivers, IriError, IriInput, IriProvenance, IriResult, IriVersion,
    };
}

/// Direct HWM model API, re-exported without data-store access.
#[cfg(feature = "hwm")]
pub mod hwm {
    pub use ionoray_hwm::{
        Hwm, HwmError, HwmGeomagneticActivity, HwmInput, HwmProvenance, HwmResult, HwmVersion,
    };
}

/// Direct NRLMSIS model API, re-exported without data-store access.
#[cfg(feature = "msis")]
pub mod msis {
    pub use ionoray_msis::{
        Msis, MsisApHistory, MsisDrivers, MsisError, MsisGeomagneticActivity, MsisInput,
        MsisProvenance, MsisResult, MsisVersion,
    };
}
#[cfg(all(feature = "indices", feature = "hwm"))]
pub use prepare_hwm::{HwmEvaluation, HwmRequest, PreparedHwm};
#[cfg(all(feature = "indices", feature = "iri"))]
pub use prepare_iri::{
    IriDriverOverrides, IriEvaluation, IriIndexEvidence, IriRequest, PreparedIri,
};
#[cfg(all(feature = "indices", feature = "msis"))]
pub use prepare_msis::{
    MsisDriverOverrides, MsisEvaluation, MsisIndexEvidence, MsisRequest, PreparedMsis,
};
#[cfg(feature = "indices")]
pub use runner::Geospace;

/// Capabilities compiled into this build.
pub const fn capabilities() -> &'static [&'static str] {
    &[
        #[cfg(feature = "indices")]
        "indices",
        #[cfg(feature = "igrf")]
        "igrf",
        #[cfg(feature = "iri")]
        "iri",
        #[cfg(feature = "hwm")]
        "hwm",
        #[cfg(feature = "msis")]
        "msis",
    ]
}
