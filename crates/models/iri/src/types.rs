use ionoray_core::{
    QueryPoint,
    units::{Angle, Length, ThermodynamicTemperature},
};
use serde::{Deserialize, Serialize};

/// IRI scientific release implemented by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IriVersion {
    /// International Reference Ionosphere 2020, pinned to the 2025-09-25 code snapshot.
    Iri2020,
}

/// Explicit empirical drivers; the model never reads mutable index files.
/// Internal neutral-atmosphere magnetic response is disabled; no Ap is inferred.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriDrivers {
    /// 12-month running mean sunspot number Rz12.
    pub sunspot_number_12_month: f64,
    /// 12-month running mean ionospheric IG index, which may be negative.
    pub ionospheric_index_12_month: f64,
    /// Daily adjusted F10.7 solar flux in solar flux units.
    pub f107_daily: f64,
    /// Centered 81-day adjusted F10.7 average in solar flux units.
    pub f107_81_day: f64,
}

/// Explicit input for one IRI plasma evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriInput {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Caller-supplied solar and ionospheric drivers.
    pub drivers: IriDrivers,
}

/// Absolute ion number densities in m^-3.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IonComposition {
    /// O+ number density.
    pub oxygen_m3: Option<f64>,
    /// H+ number density.
    pub hydrogen_m3: Option<f64>,
    /// He+ number density.
    pub helium_m3: Option<f64>,
    /// O2+ number density.
    pub molecular_oxygen_m3: Option<f64>,
    /// NO+ number density.
    pub nitric_oxide_m3: Option<f64>,
    /// Cluster-ion number density.
    pub cluster_m3: Option<f64>,
    /// N+ number density.
    pub nitrogen_m3: Option<f64>,
}

/// Plasma and thermal state at one geodetic point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IonospherePoint {
    /// Electron number density in m^-3.
    pub electron_density_m3: Option<f64>,
    /// Neutral temperature when available.
    pub neutral_temperature: Option<ThermodynamicTemperature>,
    /// Ion temperature when available.
    pub ion_temperature: Option<ThermodynamicTemperature>,
    /// Electron temperature when available.
    pub electron_temperature: Option<ThermodynamicTemperature>,
    /// Absolute densities for the official ion species.
    pub ions: IonComposition,
    /// Solar zenith angle.
    pub solar_zenith_angle: Angle,
    /// IGRF magnetic dip angle.
    pub magnetic_dip_angle: Angle,
    /// Modified dip latitude.
    pub modified_dip_latitude: Angle,
}

/// Peak density and height for one modeled ionospheric layer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriLayerPeak {
    /// Peak electron number density in m^-3.
    pub electron_density_m3: f64,
    /// Peak height above the reference ellipsoid.
    pub height: Length,
}

/// F2, F1, and E layer peaks reported by IRI.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriLayerPeaks {
    /// F2 peak, when the backend reports one.
    pub f2: Option<IriLayerPeak>,
    /// F1 peak, which may not exist at the requested time and location.
    pub f1: Option<IriLayerPeak>,
    /// E peak, when the backend reports one.
    pub e: Option<IriLayerPeak>,
}

/// Exact implementation and official-asset identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IriProvenance {
    /// Scientific model release.
    pub version: IriVersion,
    /// SHA-256 of the pinned official IRI release archive.
    pub release_sha256: String,
    /// SHA-256 identity of the coefficient and IGRF asset manifest.
    pub asset_set_sha256: String,
    /// Whether Cargo downloaded the release or used a supplied official directory.
    pub build_source: String,
    /// Rust package version implementing the wrapper.
    pub implementation_version: String,
}

/// Ionosphere state and layer peaks paired with required provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IriResult {
    /// Plasma state at the requested altitude.
    pub point: IonospherePoint,
    /// Modeled ionospheric layer peaks.
    pub peaks: IriLayerPeaks,
    /// Exact model and asset identity.
    pub provenance: IriProvenance,
}
