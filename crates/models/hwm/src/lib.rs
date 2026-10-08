//! Independent HWM neutral-wind model family with an optional official HWM14 backend.
//!
//! Enabling the default `hwm14` feature downloads and builds the official NRL
//! supplemental release during compilation. Runtime evaluation performs no
//! network, database, environment-variable, or `IONORAY_HOME` access.

mod error;
mod model;

#[cfg(feature = "hwm14")]
mod backend;

pub use error::HwmError;
pub use model::{
    HorizontalWind, Hwm, HwmGeomagneticActivity, HwmInput, HwmProvenance, HwmResult, HwmVersion,
};

/// Official HWM14 release identifier.
pub const HWM14_RELEASE: &str = "HWM14.123114";

/// Canonical citation for the HWM14 quiet-time formulation.
pub const HWM14_CITATION: &str = "Drob et al. (2015), An update to the Horizontal Wind Model (HWM): The quiet time thermosphere, Earth and Space Science, doi:10.1002/2014EA000089";
