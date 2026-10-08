//! Stable domain types shared by `IonoRay` data and model crates.

mod digest;
mod position;
mod provenance;
mod query;
mod version;

pub use digest::{DigestError, Sha256Digest};
pub use hifitime::{Duration, Epoch, TimeScale};
pub use position::{GeodeticPosition, PositionError};
pub use provenance::{DataProvenance, ModelProvenance};
pub use query::QueryPoint;
pub use version::VersionPolicy;

/// SI quantity types and units used by the public API.
pub mod units {
    pub use uom::si::angle::{degree, radian};
    pub use uom::si::f64::{
        Angle, Length, MagneticFluxDensity, ThermodynamicTemperature, Velocity,
    };
    pub use uom::si::length::{kilometer, meter};
    pub use uom::si::magnetic_flux_density::{nanotesla, tesla};
    pub use uom::si::thermodynamic_temperature::kelvin;
    pub use uom::si::velocity::meter_per_second;
}
