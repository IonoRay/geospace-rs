//! Independent IRI ionosphere model family with an optional official IRI-2020 backend.
//!
//! The model is synchronous, accepts explicit solar/ionospheric drivers, and
//! performs no runtime network, data-store, or `IONORAY_HOME` access.

#[cfg(feature = "iri2020")]
mod backend;
mod error;
mod model;
mod types;

pub use error::IriError;
pub use model::Iri;
pub use types::{
    IonComposition, IonospherePoint, IriDrivers, IriInput, IriLayerPeak, IriLayerPeaks,
    IriProvenance, IriResult, IriVersion,
};

/// License notice shipped by the official IRI Working Group distribution.
pub const IRI2020_LICENSE: &str = include_str!("../IRI-LICENSE.txt");

/// Recommended scientific citation for the IRI-2020 release.
pub const IRI2020_CITATION: &str = "Bilitza et al. (2022), The International Reference Ionosphere Model: A Review and Description of an Ionospheric Benchmark, Reviews of Geophysics 60(4), e2022RG000792, doi:10.1029/2022RG000792";
