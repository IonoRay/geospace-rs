#[cfg(feature = "nrlmsis21")]
use std::time::Instant;

use ionoray_core::{QueryPoint, units::ThermodynamicTemperature};
#[cfg(feature = "nrlmsis21")]
use ionoray_core::{
    TimeScale,
    units::{kelvin, kilometer},
};
use serde::{Deserialize, Serialize};
use tracing::warn;
#[cfg(feature = "nrlmsis21")]
use tracing::{debug, debug_span};

use crate::MsisError;

/// NRLMSIS scientific release implemented by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MsisVersion {
    /// NRLMSIS 2.1, released 4 April 2022.
    Nrlmsis21,
}

/// Storm-time Ap history required by the NRLMSIS 2.1 interface.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisApHistory {
    /// Daily Ap.
    pub daily: f64,
    /// Current three-hour ap.
    pub current: f64,
    /// Three-hour ap one bin earlier.
    pub three_hours_ago: f64,
    /// Three-hour ap two bins earlier.
    pub six_hours_ago: f64,
    /// Three-hour ap three bins earlier.
    pub nine_hours_ago: f64,
    /// Mean of the eight bins 12-33 hours earlier.
    pub average_12_to_33_hours: f64,
    /// Mean of the eight bins 36-57 hours earlier.
    pub average_36_to_57_hours: f64,
}

impl MsisApHistory {
    pub(crate) const fn values(self) -> [f64; 7] {
        [
            self.daily,
            self.current,
            self.three_hours_ago,
            self.six_hours_ago,
            self.nine_hours_ago,
            self.average_12_to_33_hours,
            self.average_36_to_57_hours,
        ]
    }
}

/// Explicit external drivers; the model never queries an index provider.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisDrivers {
    /// Centered 81-day F10.7 average.
    pub f107a: f64,
    /// Previous-day F10.7.
    pub f107_previous_day: f64,
    /// Daily-only or complete storm-time geomagnetic input.
    pub geomagnetic_activity: MsisGeomagneticActivity,
}

/// Selects the official daily-Ap or storm-time Ap formulation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum MsisGeomagneticActivity {
    /// Use only daily Ap, matching the official default.
    Daily(f64),
    /// Use the full seven-element storm-time history.
    StormTime(MsisApHistory),
}

impl MsisGeomagneticActivity {
    #[cfg(feature = "nrlmsis21")]
    pub(crate) const fn is_storm_time(self) -> bool {
        matches!(self, Self::StormTime(_))
    }

    pub(crate) const fn values(self) -> [f64; 7] {
        match self {
            Self::Daily(value) => [value; 7],
            Self::StormTime(history) => history.values(),
        }
    }
}

/// Explicit input for one neutral-atmosphere evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisInput {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Caller-supplied solar and geomagnetic drivers.
    pub drivers: MsisDrivers,
}

/// NRLMSIS neutral temperature and density output in documented SI units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NeutralAtmosphere {
    /// Neutral temperature at altitude.
    pub temperature: ThermodynamicTemperature,
    /// Exospheric temperature.
    pub exospheric_temperature: ThermodynamicTemperature,
    /// Total mass density in kg/m3.
    pub mass_density_kg_m3: Option<f64>,
    /// N2 number density in m-3.
    pub n2_number_density_m3: Option<f64>,
    /// O2 number density in m-3.
    pub o2_number_density_m3: Option<f64>,
    /// O number density in m-3.
    pub o_number_density_m3: Option<f64>,
    /// He number density in m-3.
    pub he_number_density_m3: Option<f64>,
    /// H number density in m-3.
    pub h_number_density_m3: Option<f64>,
    /// Ar number density in m-3.
    pub ar_number_density_m3: Option<f64>,
    /// N number density in m-3.
    pub n_number_density_m3: Option<f64>,
    /// Anomalous oxygen number density in m-3.
    pub anomalous_o_number_density_m3: Option<f64>,
    /// NO number density in m-3.
    pub no_number_density_m3: Option<f64>,
}

/// Exact implementation and official-asset identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MsisProvenance {
    /// Scientific model release.
    pub version: MsisVersion,
    /// SHA-256 of the official NRL release archive.
    pub release_sha256: String,
    /// SHA-256 of the official binary parameter file.
    pub parameter_sha256: String,
    /// Whether Cargo downloaded the release or used a supplied official directory.
    pub build_source: String,
    /// Rust package version implementing the wrapper.
    pub implementation_version: String,
}

/// Neutral-atmosphere result paired with required provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MsisResult {
    /// Evaluated atmosphere.
    pub atmosphere: NeutralAtmosphere,
    /// Exact model and asset identity.
    pub provenance: MsisProvenance,
}

/// Independent NRLMSIS model handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Msis {
    version: MsisVersion,
}

impl Msis {
    /// Creates a model handle without runtime network or data-store access.
    pub const fn new(version: MsisVersion) -> Self {
        Self { version }
    }

    /// Returns the selected scientific release.
    pub const fn version(self) -> MsisVersion {
        self.version
    }

    /// Evaluates NRLMSIS 2.1 with explicit external drivers.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid drivers, below-surface altitude, unavailable
    /// backend, parameter materialization failure, or backend initialization.
    pub fn evaluate(&self, input: &MsisInput) -> Result<MsisResult, MsisError> {
        #[cfg(not(feature = "nrlmsis21"))]
        {
            validate_drivers(input.drivers)?;
            Err(MsisError::BackendUnavailable)
        }
        #[cfg(feature = "nrlmsis21")]
        {
            self.evaluate_nrlmsis21(input)
        }
    }

    #[cfg(feature = "nrlmsis21")]
    fn evaluate_nrlmsis21(self, input: &MsisInput) -> Result<MsisResult, MsisError> {
        validate_drivers(input.drivers)?;
        let altitude_km = input.query.position.altitude().get::<kilometer>();
        if altitude_km < 0.0 {
            return Err(MsisError::AltitudeBelowSurface(altitude_km));
        }
        let started = Instant::now();
        let utc = input.query.epoch.to_time_scale(TimeScale::UTC);
        let day_of_year = utc.day_of_year().floor();
        let (_, _, _, hour, minute, second, nanosecond) = utc.to_gregorian_utc();
        let ut_seconds = f64::from(hour) * 3_600.0
            + f64::from(minute) * 60.0
            + f64::from(second)
            + f64::from(nanosecond) * 1.0e-9;
        let span = debug_span!(
            "msis.evaluate",
            model = "nrlmsis21",
            pid = std::process::id(),
            day_of_year,
            ut_seconds,
            altitude_km,
            storm_time = input.drivers.geomagnetic_activity.is_storm_time(),
            release_sha256 = env!("IONORAY_NRLMSIS21_RELEASE_SHA256"),
            parameter_sha256 = env!("IONORAY_NRLMSIS21_PARAMETER_SHA256")
        );
        let _entered = span.enter();

        let raw = crate::backend::evaluate(input, day_of_year, ut_seconds)?;
        let atmosphere = NeutralAtmosphere {
            temperature: ThermodynamicTemperature::new::<kelvin>(raw.temperature),
            exospheric_temperature: ThermodynamicTemperature::new::<kelvin>(
                raw.exospheric_temperature,
            ),
            mass_density_kg_m3: present(raw.densities[0]),
            n2_number_density_m3: present(raw.densities[1]),
            o2_number_density_m3: present(raw.densities[2]),
            o_number_density_m3: present(raw.densities[3]),
            he_number_density_m3: present(raw.densities[4]),
            h_number_density_m3: present(raw.densities[5]),
            ar_number_density_m3: present(raw.densities[6]),
            n_number_density_m3: present(raw.densities[7]),
            anomalous_o_number_density_m3: present(raw.densities[8]),
            no_number_density_m3: present(raw.densities[9]),
        };
        debug!(
            event = "msis.evaluation.completed",
            temperature_k = raw.temperature,
            mass_density_kg_m3 = raw.densities[0],
            elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            "evaluated NRLMSIS 2.1"
        );
        Ok(MsisResult {
            atmosphere,
            provenance: MsisProvenance {
                version: self.version,
                release_sha256: format!("sha256:{}", env!("IONORAY_NRLMSIS21_RELEASE_SHA256")),
                parameter_sha256: format!("sha256:{}", env!("IONORAY_NRLMSIS21_PARAMETER_SHA256")),
                build_source: env!("IONORAY_NRLMSIS21_SOURCE_MODE").to_owned(),
                implementation_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        })
    }
}

fn validate_drivers(drivers: MsisDrivers) -> Result<(), MsisError> {
    validate("f107a", drivers.f107a)?;
    validate("f107_previous_day", drivers.f107_previous_day)?;
    for (name, value) in [
        "ap_daily",
        "ap_current",
        "ap_3h_ago",
        "ap_6h_ago",
        "ap_9h_ago",
        "ap_average_12_to_33h",
        "ap_average_36_to_57h",
    ]
    .into_iter()
    .zip(drivers.geomagnetic_activity.values())
    {
        validate(name, value)?;
    }
    Ok(())
}

fn validate(name: &'static str, value: f64) -> Result<(), MsisError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        warn!(
            event = "msis.input.rejected",
            name, value, "rejected NRLMSIS driver"
        );
        Err(MsisError::InvalidDriver { name, value })
    }
}

#[cfg(feature = "nrlmsis21")]
fn present(value: f64) -> Option<f64> {
    (value > 1.0e-37).then_some(value)
}

#[cfg(test)]
mod tests;
