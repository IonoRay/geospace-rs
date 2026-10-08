use std::{sync::Arc, time::Instant};

use ionoray_core::{
    QueryPoint, Sha256Digest,
    units::{Angle, MagneticFluxDensity, degree, kilometer, nanotesla, radian},
};
use serde::{Deserialize, Serialize};
use tracing::{debug, debug_span, warn};

use crate::{
    IgrfError,
    coefficients::{CoefficientSet, ResolutionMode},
    solver::synthesize,
    time::decimal_year,
};

/// IGRF scientific release implemented by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IgrfVersion {
    /// Fourteenth-generation International Geomagnetic Reference Field.
    Igrf14,
}

/// Explicit input for one geomagnetic field evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct IgrfInput {
    /// Evaluation point and epoch.
    pub query: QueryPoint,
}

/// Local east-north-up geomagnetic field components and derived elements.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MagneticField {
    /// East component.
    pub east: MagneticFluxDensity,
    /// North component.
    pub north: MagneticFluxDensity,
    /// Up component.
    pub up: MagneticFluxDensity,
    /// Total field intensity.
    pub magnitude: MagneticFluxDensity,
    /// Declination, positive east of true north.
    pub declination: Angle,
    /// Inclination, positive downward.
    pub inclination: Angle,
}

/// Immutable record of the exact model and coefficients used.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IgrfProvenance {
    /// Scientific model release.
    pub version: IgrfVersion,
    /// SHA-256 identity of the embedded official coefficient table.
    pub coefficient_sha256: Sha256Digest,
    /// Rust package version implementing the solver.
    pub implementation_version: String,
    /// Spherical-harmonic degree selected for the epoch.
    pub max_degree: usize,
}

/// Model output paired with its required provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IgrfResult {
    /// Evaluated field.
    pub field: MagneticField,
    /// Exact implementation and asset identity.
    pub provenance: IgrfProvenance,
}

/// IGRF model handle with an explicit scientific version.
#[derive(Clone, Debug)]
pub struct Igrf {
    version: IgrfVersion,
    coefficients: Arc<CoefficientSet>,
}

impl Igrf {
    /// Creates an independent model from the embedded official coefficients.
    ///
    /// # Errors
    ///
    /// Returns an error if the compile-time coefficient asset is malformed or
    /// incomplete, or differs from the pinned official SHA-256.
    pub fn new(version: IgrfVersion) -> Result<Self, IgrfError> {
        Ok(Self {
            version,
            coefficients: Arc::new(CoefficientSet::load()?),
        })
    }

    /// Returns the selected scientific release.
    pub const fn version(&self) -> IgrfVersion {
        self.version
    }

    /// Evaluates the geomagnetic main field at one WGS84 geodetic point.
    ///
    /// Height is above the ellipsoid. UTC epochs are converted to decimal years
    /// using elapsed seconds divided by the duration of that UTC calendar year.
    /// The supported interval is inclusive at 1900.0 and 2030.0 (not all of 2030).
    /// Components are east/north/up SI quantities; inclination is positive down.
    ///
    /// # Errors
    ///
    /// Returns an error when the epoch is outside 1900-2030 or the altitude
    /// cannot produce a valid geocentric radius.
    pub fn evaluate(&self, input: &IgrfInput) -> Result<IgrfResult, IgrfError> {
        let started = Instant::now();
        let latitude_deg = input.query.position.latitude().get::<degree>();
        let longitude_deg = input.query.position.longitude().get::<degree>();
        let altitude_km = input.query.position.altitude().get::<kilometer>();
        let year = decimal_year(input.query.epoch);
        let span = debug_span!(
            "igrf.evaluate",
            model = "igrf14",
            pid = std::process::id(),
            coefficient_sha256 = %self.coefficients.digest,
            decimal_year = year,
            latitude_deg,
            longitude_deg,
            altitude_km
        );
        let _entered = span.enter();
        if !(1900.0..=2030.0).contains(&year) {
            warn!(
                event = "igrf.evaluation.rejected",
                reason = "epoch_out_of_range",
                "rejected IGRF input"
            );
            return Err(IgrfError::EpochOutOfRange(year));
        }
        if altitude_km <= -6_300.0 {
            warn!(
                event = "igrf.evaluation.rejected",
                reason = "altitude_out_of_range",
                "rejected IGRF input"
            );
            return Err(IgrfError::AltitudeOutOfRange(altitude_km));
        }

        let resolved = self.coefficients.resolve(year);
        match resolved.mode {
            ResolutionMode::Interpolated {
                lower_year,
                upper_year,
            } => debug!(
                event = "igrf.coefficients.resolved",
                mode = "interpolation",
                lower_year,
                upper_year,
                max_degree = resolved.max_degree,
                "resolved IGRF coefficients"
            ),
            ResolutionMode::SecularVariation { base_year } => debug!(
                event = "igrf.coefficients.resolved",
                mode = "secular_variation",
                base_year,
                max_degree = resolved.max_degree,
                "resolved IGRF coefficients"
            ),
        }
        let compute_span = debug_span!(
            "model.compute",
            model = "igrf14",
            elapsed_us = tracing::field::Empty
        );
        let compute_started = Instant::now();
        let raw = compute_span.in_scope(|| {
            synthesize(
                &resolved.values,
                resolved.max_degree,
                latitude_deg,
                longitude_deg,
                altitude_km,
            )
        });
        compute_span.record(
            "elapsed_us",
            u64::try_from(compute_started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        let horizontal = raw.north.hypot(raw.east);
        let magnitude = horizontal.hypot(raw.down);
        let field = MagneticField {
            east: MagneticFluxDensity::new::<nanotesla>(raw.east),
            north: MagneticFluxDensity::new::<nanotesla>(raw.north),
            up: MagneticFluxDensity::new::<nanotesla>(-raw.down),
            magnitude: MagneticFluxDensity::new::<nanotesla>(magnitude),
            declination: Angle::new::<radian>(raw.east.atan2(raw.north)),
            inclination: Angle::new::<radian>(raw.down.atan2(horizontal)),
        };
        debug!(
            event = "igrf.evaluation.completed",
            north_nt = raw.north,
            east_nt = raw.east,
            down_nt = raw.down,
            magnitude_nt = magnitude,
            elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            "evaluated IGRF main field"
        );
        Ok(IgrfResult {
            field,
            provenance: IgrfProvenance {
                version: self.version,
                coefficient_sha256: self.coefficients.digest,
                implementation_version: env!("CARGO_PKG_VERSION").to_owned(),
                max_degree: resolved.max_degree,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use ionoray_core::{Epoch, GeodeticPosition};

    use super::*;

    #[derive(Deserialize)]
    struct ReferenceCase {
        year: i32,
        month: u8,
        day: u8,
        decimal_year: f64,
        latitude_deg: f64,
        longitude_deg: f64,
        altitude_km: f64,
        north_nt: f64,
        east_nt: f64,
        down_nt: f64,
        magnitude_nt: f64,
        declination_deg: f64,
        inclination_deg: f64,
    }

    #[test]
    fn matches_reproducible_official_reference_matrix() {
        // Generated only by the pinned upstream interactive Fortran program.
        // See data/README.md and scripts/verify_igrf14_reference.py.
        let cases: Vec<ReferenceCase> =
            serde_json::from_str(include_str!("../data/reference.json")).unwrap();
        assert_eq!(cases.len(), 9);
        let model = Igrf::new(IgrfVersion::Igrf14).unwrap();
        for case in cases {
            let mut query = input(
                case.year,
                case.latitude_deg,
                case.longitude_deg,
                case.altitude_km,
            );
            query.query.epoch =
                Epoch::maybe_from_gregorian_utc(case.year, case.month, case.day, 0, 0, 0, 0)
                    .unwrap();
            assert!((decimal_year(query.query.epoch) - case.decimal_year).abs() < 1.0e-12);
            let result = model.evaluate(&query).unwrap();
            let field = result.field;
            for (name, actual, expected, tolerance) in [
                ("north", field.north.get::<nanotesla>(), case.north_nt, 0.5),
                ("east", field.east.get::<nanotesla>(), case.east_nt, 0.5),
                ("down", -field.up.get::<nanotesla>(), case.down_nt, 0.5),
                (
                    "magnitude",
                    field.magnitude.get::<nanotesla>(),
                    case.magnitude_nt,
                    0.5,
                ),
                (
                    "declination",
                    field.declination.get::<degree>(),
                    case.declination_deg,
                    1.0 / 120.0,
                ),
                (
                    "inclination",
                    field.inclination.get::<degree>(),
                    case.inclination_deg,
                    1.0 / 120.0,
                ),
            ] {
                assert!(
                    (actual - expected).abs() <= tolerance,
                    "year {} {name}: {actual} vs {expected}",
                    case.decimal_year
                );
            }
            assert_eq!(
                result.provenance.max_degree,
                if case.year < 1995 { 10 } else { 13 }
            );
        }
    }

    fn input(year: i32, latitude: f64, longitude: f64, altitude: f64) -> IgrfInput {
        IgrfInput {
            query: QueryPoint {
                epoch: Epoch::maybe_from_gregorian_utc(year, 1, 1, 0, 0, 0, 0).unwrap(),
                position: GeodeticPosition::from_degrees_kilometers(latitude, longitude, altitude)
                    .unwrap(),
            },
        }
    }

    #[test]
    fn rejects_epoch_outside_published_interval() {
        let model = Igrf::new(IgrfVersion::Igrf14).unwrap();
        for year in [1899, 2031] {
            assert!(matches!(
                model.evaluate(&input(year, 0.0, 0.0, 0.0)),
                Err(IgrfError::EpochOutOfRange(_))
            ));
        }
        let mut after_end = input(2030, 0.0, 0.0, 0.0);
        after_end.query.epoch = Epoch::maybe_from_gregorian_utc(2030, 1, 1, 0, 0, 1, 0).unwrap();
        assert!(matches!(
            model.evaluate(&after_end),
            Err(IgrfError::EpochOutOfRange(_))
        ));
    }

    #[test]
    fn longitude_normalization_is_model_invariant() {
        let model = Igrf::new(IgrfVersion::Igrf14).unwrap();
        let left = model.evaluate(&input(2025, 30.0, -120.0, 300.0)).unwrap();
        let right = model.evaluate(&input(2025, 30.0, 240.0, 300.0)).unwrap();
        assert_eq!(left.field, right.field);
    }

    #[test]
    fn matches_official_igrf14_fortran_reference_case() {
        let model = Igrf::new(IgrfVersion::Igrf14).unwrap();
        let result = model.evaluate(&input(2025, 30.0, 120.0, 300.0)).unwrap();
        let field = result.field;

        // Official IGRF14SYN output is rounded to the nearest nanotesla.
        assert!((field.north.get::<nanotesla>() - 29_180.0).abs() <= 0.5);
        assert!((field.east.get::<nanotesla>() - (-2_703.0)).abs() <= 0.5);
        assert!((field.up.get::<nanotesla>() - (-29_642.0)).abs() <= 0.5);
        assert!((field.magnitude.get::<nanotesla>() - 41_682.0).abs() <= 0.5);
        assert!((field.declination.get::<degree>() - (-5.3)).abs() <= 1.0 / 120.0);
        assert!((field.inclination.get::<degree>() - (45.0 + 20.0 / 60.0)).abs() <= 1.0 / 120.0);
    }

    #[test]
    fn matches_official_fortran_across_model_epochs_and_poles() {
        let model = Igrf::new(IgrfVersion::Igrf14).unwrap();
        let cases = [
            (1900, 0.0, 0.0, 0.0, 28_028.0, -8_560.0, -5_590.0, 29_834.0),
            (
                1990, -45.0, -75.0, 100.0, 20_255.0, 5_058.0, -20_196.0, 29_047.0,
            ),
            (2000, 90.0, 0.0, 0.0, 1_812.0, -875.0, 56_290.0, 56_326.0),
            (
                2030, -90.0, 179.0, 500.0, -10_207.0, 6_904.0, -41_042.0, 42_852.0,
            ),
        ];
        for (year, lat, lon, alt, north, east, down, magnitude) in cases {
            let field = model.evaluate(&input(year, lat, lon, alt)).unwrap().field;
            assert!((field.north.get::<nanotesla>() - north).abs() <= 0.5);
            assert!((field.east.get::<nanotesla>() - east).abs() <= 0.5);
            assert!((field.up.get::<nanotesla>() + down).abs() <= 0.5);
            assert!((field.magnitude.get::<nanotesla>() - magnitude).abs() <= 0.5);
        }
    }
}
