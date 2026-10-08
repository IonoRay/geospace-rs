#[cfg(feature = "iri2020")]
use std::time::Instant;

#[cfg(feature = "iri2020")]
use ionoray_core::{
    TimeScale,
    units::{Angle, Length, ThermodynamicTemperature, degree, kelvin, kilometer},
};
use tracing::warn;
#[cfg(feature = "iri2020")]
use tracing::{debug, debug_span};

#[cfg(feature = "iri2020")]
use crate::{IonComposition, IonospherePoint, IriLayerPeak, IriLayerPeaks, IriProvenance};
use crate::{IriDrivers, IriError, IriInput, IriResult, IriVersion};

/// Independent IRI model handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Iri {
    version: IriVersion,
}

impl Iri {
    /// Creates a model handle without runtime network or data-store access.
    pub const fn new(version: IriVersion) -> Self {
        Self { version }
    }

    /// Returns the selected scientific release.
    pub const fn version(self) -> IriVersion {
        self.version
    }

    /// Materializes and validates embedded assets, then initializes IRI once.
    ///
    /// Calling this method is optional: [`Self::evaluate`] invokes the same
    /// idempotent initialization path.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend is unavailable or its assets cannot be
    /// materialized, verified, or registered with the Fortran backend.
    pub fn initialize(&self) -> Result<(), IriError> {
        #[cfg(not(feature = "iri2020"))]
        {
            Err(IriError::BackendUnavailable)
        }
        #[cfg(feature = "iri2020")]
        {
            crate::backend::initialize()
        }
    }

    /// Evaluates the recommended IRI-2020 climatology at one point.
    ///
    /// Magnetic-storm, drift, spread-F, auroral-boundary, and sporadic-E
    /// extensions are intentionally disabled because this low-level model API
    /// accepts only explicit drivers and never reads mutable index files.
    /// Internal NRLMSIS00 magnetic response is also disabled (`SWMI(9)=0`);
    /// this is a four-driver climatology, not a zero-Ap or storm-time scenario.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid drivers, unsupported altitude or epoch,
    /// unavailable backend, asset preparation failure, or backend failure.
    pub fn evaluate(&self, input: &IriInput) -> Result<IriResult, IriError> {
        validate_drivers(input.drivers)?;
        #[cfg(not(feature = "iri2020"))]
        {
            Err(IriError::BackendUnavailable)
        }
        #[cfg(feature = "iri2020")]
        {
            self.evaluate_iri2020(input)
        }
    }

    #[cfg(feature = "iri2020")]
    fn evaluate_iri2020(self, input: &IriInput) -> Result<IriResult, IriError> {
        let altitude_km = input.query.position.altitude().get::<kilometer>();
        if !(60.0..=1500.0).contains(&altitude_km) {
            return Err(IriError::AltitudeOutOfRange(altitude_km));
        }
        let utc = input.query.epoch.to_time_scale(TimeScale::UTC);
        let (year, month, day, hour, minute, second, nanosecond) = utc.to_gregorian_utc();
        if !(1958..=2030).contains(&year) {
            return Err(IriError::EpochOutOfRange(year));
        }
        let ut_hours = f64::from(hour)
            + f64::from(minute) / 60.0
            + (f64::from(second) + f64::from(nanosecond) * 1.0e-9) / 3_600.0;
        let mmdd = i32::from(month) * 100 + i32::from(day);
        let started = Instant::now();
        let span = debug_span!(
            "iri.evaluate",
            model = "iri2020",
            pid = std::process::id(),
            year,
            mmdd,
            ut_hours,
            altitude_km,
            release_sha256 = env!("IONORAY_IRI2020_RELEASE_SHA256"),
            asset_set_sha256 = env!("IONORAY_IRI2020_ASSET_SET_SHA256")
        );
        let _entered = span.enter();

        let raw = crate::backend::evaluate(input, year, mmdd, ut_hours)?;
        let point = IonospherePoint {
            electron_density_m3: present(raw[0]),
            neutral_temperature: temperature(raw[1]),
            ion_temperature: temperature(raw[2]),
            electron_temperature: temperature(raw[3]),
            ions: IonComposition {
                oxygen_m3: present(raw[4]),
                hydrogen_m3: present(raw[5]),
                helium_m3: present(raw[6]),
                molecular_oxygen_m3: present(raw[7]),
                nitric_oxide_m3: present(raw[8]),
                cluster_m3: present(raw[9]),
                nitrogen_m3: present(raw[10]),
            },
            solar_zenith_angle: Angle::new::<degree>(raw[17]),
            magnetic_dip_angle: Angle::new::<degree>(raw[18]),
            modified_dip_latitude: Angle::new::<degree>(raw[19]),
        };
        let peaks = IriLayerPeaks {
            f2: peak(raw[11], raw[12]),
            f1: peak(raw[13], raw[14]),
            e: peak(raw[15], raw[16]),
        };
        debug!(
            event = "iri.evaluation.completed",
            electron_density_m3 = point.electron_density_m3,
            electron_temperature_k = point
                .electron_temperature
                .map(|value| value.get::<kelvin>()),
            elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            "evaluated IRI-2020"
        );
        Ok(IriResult {
            point,
            peaks,
            provenance: IriProvenance {
                version: self.version,
                release_sha256: format!("sha256:{}", env!("IONORAY_IRI2020_RELEASE_SHA256")),
                asset_set_sha256: format!("sha256:{}", env!("IONORAY_IRI2020_ASSET_SET_SHA256")),
                build_source: env!("IONORAY_IRI2020_SOURCE_MODE").to_owned(),
                implementation_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        })
    }
}

fn validate_drivers(drivers: IriDrivers) -> Result<(), IriError> {
    for (name, value) in [
        ("sunspot_number_12_month", drivers.sunspot_number_12_month),
        (
            "ionospheric_index_12_month",
            drivers.ionospheric_index_12_month,
        ),
        ("f107_daily", drivers.f107_daily),
        ("f107_81_day", drivers.f107_81_day),
    ] {
        if !value.is_finite() {
            return Err(invalid_driver(name, "finite", value));
        }
    }
    for (name, value) in [
        ("sunspot_number_12_month", drivers.sunspot_number_12_month),
        ("f107_daily", drivers.f107_daily),
        ("f107_81_day", drivers.f107_81_day),
    ] {
        if value < 0.0 {
            return Err(invalid_driver(name, "finite and non-negative", value));
        }
    }
    Ok(())
}

fn invalid_driver(name: &'static str, constraint: &'static str, value: f64) -> IriError {
    warn!(
        event = "iri.input.rejected",
        name, constraint, value, "rejected IRI driver"
    );
    IriError::InvalidDriver {
        name,
        constraint,
        value,
    }
}

#[cfg(feature = "iri2020")]
fn present(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[cfg(feature = "iri2020")]
fn temperature(value: f64) -> Option<ThermodynamicTemperature> {
    present(value).map(ThermodynamicTemperature::new::<kelvin>)
}

#[cfg(feature = "iri2020")]
fn peak(density_m3: f64, height_km: f64) -> Option<IriLayerPeak> {
    (density_m3.is_finite() && height_km.is_finite() && density_m3 > 0.0 && height_km > 0.0).then(
        || IriLayerPeak {
            electron_density_m3: density_m3,
            height: Length::new::<kilometer>(height_km),
        },
    )
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "iri2020")]
    use ionoray_core::{Epoch, GeodeticPosition, QueryPoint};

    use super::*;

    #[test]
    fn rejects_non_finite_driver() {
        assert!(matches!(
            validate_drivers(IriDrivers {
                sunspot_number_12_month: f64::NAN,
                ionospheric_index_12_month: 100.0,
                f107_daily: 120.0,
                f107_81_day: 110.0,
            }),
            Err(IriError::InvalidDriver { .. })
        ));
    }

    #[test]
    fn accepts_negative_official_ig12_but_rejects_negative_rz12() {
        let drivers = IriDrivers {
            sunspot_number_12_month: 5.0,
            ionospheric_index_12_month: -5.526_666_666_666_667,
            f107_daily: 70.0,
            f107_81_day: 70.0,
        };
        assert_eq!(validate_drivers(drivers), Ok(()));
        assert!(matches!(
            validate_drivers(IriDrivers {
                sunspot_number_12_month: -0.1,
                ..drivers
            }),
            Err(IriError::InvalidDriver {
                name: "sunspot_number_12_month",
                ..
            })
        ));
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn evaluates_official_negative_ig12() {
        let mut input = reference_input();
        input.drivers.ionospheric_index_12_month = -5.526_666_666_666_667;
        let result = Iri::new(IriVersion::Iri2020).evaluate(&input).unwrap();
        assert!(result.point.electron_density_m3.is_some());
    }

    #[cfg(not(feature = "iri2020"))]
    #[test]
    fn disabled_backend_returns_explicit_error() {
        assert_eq!(
            Iri::new(IriVersion::Iri2020).initialize(),
            Err(IriError::BackendUnavailable)
        );
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn rejects_altitude_outside_common_plasma_domain() {
        let mut input = reference_input();
        input.query.position = GeodeticPosition::from_degrees_kilometers(0.0, 0.0, 59.0).unwrap();
        assert_eq!(
            Iri::new(IriVersion::Iri2020).evaluate(&input),
            Err(IriError::AltitudeOutOfRange(59.0))
        );
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn rejects_epoch_outside_pinned_igrf_interval() {
        let mut input = reference_input();
        input.query.epoch = Epoch::maybe_from_gregorian_utc(1957, 3, 20, 12, 0, 0, 0).unwrap();
        assert_eq!(
            Iri::new(IriVersion::Iri2020).evaluate(&input),
            Err(IriError::EpochOutOfRange(1957))
        );
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn matches_pinned_official_iri2020_point_reference() {
        // Independently replayable with scripts/verify_iri2020_reference.py.
        // Single-precision upstream output; relative tolerance 1e-6.
        let result = Iri::new(IriVersion::Iri2020)
            .evaluate(&reference_input())
            .unwrap();
        let expected: [f64; 20] =
            serde_json::from_str(include_str!("../data/reference-no-ap.json")).unwrap();
        let point = result.point;
        let ions = point.ions;
        let actual = [
            point.electron_density_m3,
            point.neutral_temperature.map(|v| v.get::<kelvin>()),
            point.ion_temperature.map(|v| v.get::<kelvin>()),
            point.electron_temperature.map(|v| v.get::<kelvin>()),
            ions.oxygen_m3,
            ions.hydrogen_m3,
            ions.helium_m3,
            ions.molecular_oxygen_m3,
            ions.nitric_oxide_m3,
            ions.cluster_m3,
            ions.nitrogen_m3,
        ];
        for (actual, expected) in actual.into_iter().zip(expected) {
            if expected < 0.0 {
                assert_eq!(actual, None);
            } else {
                assert_relative(actual.unwrap(), expected, 1e-6);
            }
        }
        for (layer, pair) in [result.peaks.f2, result.peaks.f1, result.peaks.e]
            .into_iter()
            .zip(expected[11..17].as_chunks::<2>().0)
        {
            let layer = layer.unwrap();
            assert_relative(layer.electron_density_m3, pair[0], 1e-6);
            assert_relative(layer.height.get::<kilometer>(), pair[1], 1e-6);
        }
        for (angle, expected) in [
            point.solar_zenith_angle,
            point.magnetic_dip_angle,
            point.modified_dip_latitude,
        ]
        .into_iter()
        .zip(&expected[17..])
        {
            assert_relative(angle.get::<degree>(), *expected, 1e-6);
        }
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn changed_date_and_drivers_do_not_contaminate_repeated_evaluation() {
        let model = Iri::new(IriVersion::Iri2020);
        let baseline = reference_input();
        let before = model.evaluate(&baseline).unwrap();
        let mut changed = baseline;
        changed.query.epoch = Epoch::maybe_from_gregorian_utc(2020, 7, 1, 0, 0, 0, 0).unwrap();
        changed.drivers.ionospheric_index_12_month = -5.526_666_666_666_667;
        changed.drivers.f107_daily = 150.0;
        model.evaluate(&changed).unwrap();
        assert_eq!(before, model.evaluate(&baseline).unwrap());
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn unavailable_and_non_finite_outputs_remain_none() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(present(value), None);
            assert_eq!(temperature(value), None);
            assert_eq!(peak(value, 300.0), None);
            assert_eq!(peak(1.0e12, value), None);
        }
        assert_eq!(present(0.0), Some(0.0));
        assert_eq!(peak(0.0, 300.0), None);
        assert_eq!(peak(1.0e12, 0.0), None);
    }

    #[cfg(feature = "iri2020")]
    #[test]
    fn supports_closed_epoch_and_altitude_boundaries() {
        let model = Iri::new(IriVersion::Iri2020);
        let mut input = reference_input();
        for year in [1958, 2030] {
            input.query.epoch = Epoch::maybe_from_gregorian_utc(year, 3, 20, 12, 0, 0, 0).unwrap();
            for altitude in [60.0, 1500.0] {
                input.query.position =
                    GeodeticPosition::from_degrees_kilometers(0.0, 0.0, altitude).unwrap();
                let result = model.evaluate(&input).unwrap();
                assert!(result.peaks.f2.is_some());
                // irisub.for sets HNEA >= 65 km, so Ne is unavailable at 60 km.
                if altitude < 65.0 {
                    assert_eq!(result.point.electron_density_m3, None);
                }
            }
        }
        input = reference_input();
        input.query.position = GeodeticPosition::from_degrees_kilometers(0.0, 0.0, 1500.1).unwrap();
        assert!(matches!(
            model.evaluate(&input),
            Err(IriError::AltitudeOutOfRange(_))
        ));
        input = reference_input();
        input.query.epoch = Epoch::maybe_from_gregorian_utc(2031, 1, 1, 0, 0, 0, 0).unwrap();
        assert_eq!(model.evaluate(&input), Err(IriError::EpochOutOfRange(2031)));
    }

    #[cfg(feature = "iri2020")]
    fn reference_input() -> IriInput {
        IriInput {
            query: QueryPoint {
                epoch: Epoch::maybe_from_gregorian_utc(2020, 3, 20, 12, 0, 0, 0).unwrap(),
                position: GeodeticPosition::from_degrees_kilometers(0.0, 0.0, 300.0).unwrap(),
            },
            drivers: IriDrivers {
                sunspot_number_12_month: 10.0,
                ionospheric_index_12_month: 10.0,
                f107_daily: 70.0,
                f107_81_day: 70.0,
            },
        }
    }

    #[cfg(feature = "iri2020")]
    fn assert_relative(actual: f64, expected: f64, tolerance: f64) {
        let relative = (actual - expected).abs() / expected.abs().max(1.0);
        assert!(relative <= tolerance, "{actual} != {expected}");
    }
}
