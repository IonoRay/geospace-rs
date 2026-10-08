//! Independent NRLMSIS model family with an optional official 2.1 backend.
//!
//! Enabling the default `nrlmsis21` feature downloads and builds the official
//! NRL release during compilation. Runtime evaluation performs no network,
//! database, or `IONORAY_HOME` access.

mod error;
mod model;

#[cfg(feature = "nrlmsis21")]
mod backend;

pub use error::MsisError;
pub use model::{
    Msis, MsisApHistory, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisProvenance,
    MsisResult, MsisVersion, NeutralAtmosphere,
};

/// Notice required by the official NRLMSIS 2.1 license.
pub const NRLMSIS21_NOTICE: &str = "This software incorporates the MSIS empirical atmospheric model software designed and provided by NRL. Use is governed by the Open Source Academic Research License Agreement contained in nrlmsis2.1_license.txt.";

/// Returns the exact official license bytes embedded into a backend-enabled build.
///
/// The upstream text uses a legacy encoding, so the exact artifact is exposed
/// as bytes instead of performing a lossy conversion.
#[cfg(feature = "nrlmsis21")]
pub const fn nrlmsis21_license_bytes() -> &'static [u8] {
    include_bytes!(concat!(
        env!("OUT_DIR"),
        "/nrlmsis21/assets/nrlmsis2.1_license.txt"
    ))
}
