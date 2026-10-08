#[cfg(feature = "hwm14")]
use std::time::Instant;

use ionoray_core::{QueryPoint, units::Velocity};
#[cfg(feature = "hwm14")]
use ionoray_core::{
    TimeScale,
    units::{kilometer, meter_per_second},
};
use serde::{Deserialize, Serialize};
use tracing::warn;
#[cfg(feature = "hwm14")]
use tracing::{debug, debug_span};

use crate::HwmError;

/// HWM scientific release implemented by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HwmVersion {
    /// Horizontal Wind Model 2014, release HWM14.123114.
    Hwm14,
}

/// Selects quiet-time winds or total winds with DWM07 disturbance winds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum HwmGeomagneticActivity {
    /// Evaluate only the HWM14 quiet-time climatology.
    Quiet,
    /// Add DWM07 disturbance winds using the current three-hour ap index.
    Disturbed {
        /// Current three-hour ap index.
        current_ap: f64,
    },
}

impl HwmGeomagneticActivity {
    #[cfg(feature = "hwm14")]
    pub(crate) const fn ap(self) -> f64 {
        match self {
            Self::Quiet => -1.0,
            Self::Disturbed { current_ap } => current_ap,
        }
    }
}

/// Explicit input for one horizontal neutral-wind evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HwmInput {
    /// UTC epoch and WGS84 geodetic position.
    pub query: QueryPoint,
    /// Caller-selected geomagnetic activity formulation.
    pub geomagnetic_activity: HwmGeomagneticActivity,
}

/// Geographic horizontal wind components in SI velocity units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HorizontalWind {
    /// Meridional component, positive geographic northward.
    pub northward: Velocity,
    /// Zonal component, positive geographic eastward.
    pub eastward: Velocity,
}

/// Exact implementation and official-asset identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HwmProvenance {
    /// Scientific model release.
    pub version: HwmVersion,
    /// SHA-256 of the official NRL supplemental archive.
    pub release_sha256: String,
    /// SHA-256 identity of the three-file coefficient set.
    pub asset_set_sha256: String,
    /// Whether Cargo downloaded the release or used a supplied official directory.
    pub build_source: String,
    /// Rust package version implementing the wrapper.
    pub implementation_version: String,
}

/// Horizontal neutral wind paired with required provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HwmResult {
    /// Evaluated geographic horizontal wind.
    pub wind: HorizontalWind,
    /// Exact model and asset identity.
    pub provenance: HwmProvenance,
}

/// Independent HWM model handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hwm {
    version: HwmVersion,
}

impl Hwm {
    /// Creates a model handle without runtime network or data-store access.
    pub const fn new(version: HwmVersion) -> Self {
        Self { version }
    }

    /// Returns the selected scientific release.
    pub const fn version(self) -> HwmVersion {
        self.version
    }

    /// Materializes and validates embedded assets, then initializes HWM14 once.
    ///
    /// Calling this method is optional: [`Self::evaluate`] invokes the same
    /// idempotent initialization path. It is useful for startup preflight.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is unavailable or its assets cannot be
    /// materialized, verified, or loaded.
    pub fn initialize(&self) -> Result<(), HwmError> {
        #[cfg(not(feature = "hwm14"))]
        {
            Err(HwmError::BackendUnavailable)
        }
        #[cfg(feature = "hwm14")]
        {
            crate::backend::initialize()
        }
    }

    /// Evaluates HWM14 with explicit geomagnetic activity input.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid ap, below-surface altitude, unavailable
    /// backend, asset preparation failure, or backend initialization failure.
    pub fn evaluate(&self, input: &HwmInput) -> Result<HwmResult, HwmError> {
        validate_activity(input.geomagnetic_activity)?;
        #[cfg(not(feature = "hwm14"))]
        {
            let _ = input;
            Err(HwmError::BackendUnavailable)
        }
        #[cfg(feature = "hwm14")]
        {
            self.evaluate_hwm14(input)
        }
    }

    #[cfg(feature = "hwm14")]
    fn evaluate_hwm14(self, input: &HwmInput) -> Result<HwmResult, HwmError> {
        let altitude_km = input.query.position.altitude().get::<kilometer>();
        if altitude_km < 0.0 {
            return Err(HwmError::AltitudeBelowSurface(altitude_km));
        }

        let started = Instant::now();
        let utc = input.query.epoch.to_time_scale(TimeScale::UTC);
        let (year, month, day, hour, minute, second, nanosecond) = utc.to_gregorian_utc();
        let day_of_year = ordinal_day(year, month, day);
        let iyd = year.rem_euclid(100) * 1_000 + day_of_year;
        let ut_seconds = f64::from(hour) * 3_600.0
            + f64::from(minute) * 60.0
            + f64::from(second)
            + f64::from(nanosecond) * 1.0e-9;
        let span = debug_span!(
            "hwm.evaluate",
            model = "hwm14",
            pid = std::process::id(),
            iyd,
            ut_seconds,
            altitude_km,
            disturbed = matches!(
                input.geomagnetic_activity,
                HwmGeomagneticActivity::Disturbed { .. }
            ),
            release_sha256 = env!("IONORAY_HWM14_RELEASE_SHA256"),
            asset_set_sha256 = env!("IONORAY_HWM14_ASSET_SET_SHA256")
        );
        let _entered = span.enter();

        let raw = crate::backend::evaluate(input, iyd, ut_seconds)?;
        let wind = HorizontalWind {
            northward: Velocity::new::<meter_per_second>(f64::from(raw[0])),
            eastward: Velocity::new::<meter_per_second>(f64::from(raw[1])),
        };
        debug!(
            event = "hwm.evaluation.completed",
            northward_m_s = raw[0],
            eastward_m_s = raw[1],
            elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            "evaluated HWM14"
        );
        Ok(HwmResult {
            wind,
            provenance: HwmProvenance {
                version: self.version,
                release_sha256: format!("sha256:{}", env!("IONORAY_HWM14_RELEASE_SHA256")),
                asset_set_sha256: format!("sha256:{}", env!("IONORAY_HWM14_ASSET_SET_SHA256")),
                build_source: env!("IONORAY_HWM14_SOURCE_MODE").to_owned(),
                implementation_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        })
    }
}

#[cfg(feature = "hwm14")]
fn ordinal_day(year: i32, month: u8, day: u8) -> i32 {
    const DAYS_BEFORE_MONTH: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let before = DAYS_BEFORE_MONTH[usize::from(month - 1)];
    let leap_day = i32::from(month > 2 && is_leap_year(year));
    before + i32::from(day) + leap_day
}

#[cfg(feature = "hwm14")]
const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn validate_activity(activity: HwmGeomagneticActivity) -> Result<(), HwmError> {
    if let HwmGeomagneticActivity::Disturbed { current_ap } = activity
        && (!current_ap.is_finite() || current_ap < 0.0)
    {
        warn!(
            event = "hwm.input.rejected",
            current_ap, "rejected HWM14 driver"
        );
        return Err(HwmError::InvalidAp(current_ap));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "hwm14")]
    use ionoray_core::{Epoch, GeodeticPosition, units::meter_per_second};

    use super::*;

    #[test]
    fn rejects_non_finite_ap() {
        for current_ap in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(matches!(
                validate_activity(HwmGeomagneticActivity::Disturbed { current_ap }),
                Err(HwmError::InvalidAp(_))
            ));
        }
    }

    #[cfg(not(feature = "hwm14"))]
    #[test]
    fn disabled_backend_returns_explicit_error() {
        assert_eq!(
            Hwm::new(HwmVersion::Hwm14).initialize(),
            Err(HwmError::BackendUnavailable)
        );
    }

    #[cfg(feature = "hwm14")]
    #[test]
    fn matches_official_hwm14_height_profile_reference() {
        let model = Hwm::new(HwmVersion::Hwm14);
        model.initialize().unwrap();
        let query = QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(1995, 5, 30, 12, 0, 0, 0).unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(-45.0, -85.0, 250.0).unwrap(),
        };
        let quiet = model
            .evaluate(&HwmInput {
                query,
                geomagnetic_activity: HwmGeomagneticActivity::Quiet,
            })
            .unwrap();
        let total = model
            .evaluate(&HwmInput {
                query,
                geomagnetic_activity: HwmGeomagneticActivity::Disturbed { current_ap: 80.0 },
            })
            .unwrap();

        assert_close(quiet.wind.northward.get::<meter_per_second>(), -4.150);
        assert_close(quiet.wind.eastward.get::<meter_per_second>(), -68.595);
        assert_close(total.wind.northward.get::<meter_per_second>(), 40.408);
        assert_close(total.wind.eastward.get::<meter_per_second>(), -87.560);
    }

    #[cfg(feature = "hwm14")]
    #[test]
    fn matches_official_complete_height_and_latitude_profiles() {
        // Upstream checkhwm14.f90 and Check/gfortran.txt: see data/README.md.
        let model = Hwm::new(HwmVersion::Hwm14);
        let mut latitude_profile = false;
        let mut count = 0;
        for line in include_str!(concat!(env!("OUT_DIR"), "/hwm14/reference-profiles.txt")).lines()
        {
            if line.trim() == "latitude profile" {
                latitude_profile = true;
            }
            let Ok(values) = line
                .split_whitespace()
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            if values.len() != 7 {
                continue;
            }
            let (month, day, hour, lat, lon, alt, ap) = if latitude_profile {
                (11, 1, 18, values[0], 30.0, 250.0, 48.0)
            } else {
                (5, 30, 12, -45.0, -85.0, values[0], 80.0)
            };
            let query = QueryPoint {
                epoch: Epoch::maybe_from_gregorian_utc(1995, month, day, hour, 0, 0, 0).unwrap(),
                position: GeodeticPosition::from_degrees_kilometers(lat, lon, alt).unwrap(),
            };
            // Upstream "disturbed" columns are disturbance alone; our Disturbed
            // mode returns TOTAL = quiet + disturbance (columns 5 and 6).
            for (activity, north, east) in [
                (HwmGeomagneticActivity::Quiet, values[1], values[2]),
                (
                    HwmGeomagneticActivity::Disturbed { current_ap: ap },
                    values[5],
                    values[6],
                ),
            ] {
                let result = model
                    .evaluate(&HwmInput {
                        query,
                        geomagnetic_activity: activity,
                    })
                    .unwrap();
                assert_close(result.wind.northward.get::<meter_per_second>(), north);
                assert_close(result.wind.eastward.get::<meter_per_second>(), east);
                assert_eq!(
                    result.provenance.release_sha256,
                    "sha256:4de451beeadef7b3ec3aa5b91129ea98866b9e7156cecf4be1343c33a6f57978"
                );
                assert_eq!(
                    result.provenance.asset_set_sha256,
                    "sha256:8121fa349137301f99a869e9521d8e732e933893d599d1ab75f2ce198d3048bb"
                );
            }
            count += 1;
        }
        assert_eq!(count, 36);
    }

    #[cfg(feature = "hwm14")]
    #[test]
    fn evaluate_initializes_and_optional_preflight_is_idempotent() {
        let model = Hwm::new(HwmVersion::Hwm14);
        let input = HwmInput {
            query: QueryPoint {
                epoch: Epoch::maybe_from_gregorian_utc(1995, 5, 30, 12, 0, 0, 0).unwrap(),
                position: GeodeticPosition::from_degrees_kilometers(-45.0, -85.0, 250.0).unwrap(),
            },
            geomagnetic_activity: HwmGeomagneticActivity::Quiet,
        };
        let result = model.evaluate(&input).unwrap();
        model.initialize().unwrap();
        model.initialize().unwrap();
        assert_eq!(model.evaluate(&input).unwrap(), result);
    }

    #[cfg(feature = "hwm14")]
    #[test]
    fn rejects_below_surface_and_unrepresentable_ap() {
        let model = Hwm::new(HwmVersion::Hwm14);
        let mut input = HwmInput {
            query: QueryPoint {
                epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
                position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, -1.0).unwrap(),
            },
            geomagnetic_activity: HwmGeomagneticActivity::Quiet,
        };
        assert!(matches!(
            model.evaluate(&input),
            Err(HwmError::AltitudeBelowSurface(_))
        ));
        input.query.position =
            GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 250.0).unwrap();
        input.geomagnetic_activity = HwmGeomagneticActivity::Disturbed {
            current_ap: f64::MAX,
        };
        assert!(matches!(
            model.evaluate(&input),
            Err(HwmError::InputOutOfRange {
                name: "current_ap",
                ..
            })
        ));
    }

    #[cfg(feature = "hwm14")]
    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= 0.002, "{actual} != {expected}");
    }
}
