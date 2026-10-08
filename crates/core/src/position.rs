use serde::{Deserialize, Serialize};
use thiserror::Error;
use uom::si::{
    angle::{degree, radian},
    f64::{Angle, Length},
    length::kilometer,
};

/// A WGS 84-style geodetic position.
///
/// The type preserves physical units. Datum-specific transformations remain in
/// an optional coordinate adapter rather than being implicit here.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GeodeticPositionWire")]
pub struct GeodeticPosition {
    latitude: Angle,
    longitude: Angle,
    altitude: Length,
}

impl GeodeticPosition {
    /// Builds a validated position from conventional model input units.
    ///
    /// # Errors
    ///
    /// Returns [`PositionError`] when a scalar is not finite or latitude is
    /// outside the closed interval [-90°, 90°].
    #[tracing::instrument(
        name = "core.position.create",
        level = "debug",
        fields(latitude_deg, longitude_deg, altitude_km),
        err(level = "warn")
    )]
    pub fn from_degrees_kilometers(
        latitude_deg: f64,
        longitude_deg: f64,
        altitude_km: f64,
    ) -> Result<Self, PositionError> {
        if !latitude_deg.is_finite() || !longitude_deg.is_finite() || !altitude_km.is_finite() {
            return Err(PositionError::NonFinite);
        }
        if !(-90.0..=90.0).contains(&latitude_deg) {
            return Err(PositionError::LatitudeOutOfRange(latitude_deg));
        }

        GeodeticPositionWire {
            latitude: Angle::new::<degree>(latitude_deg),
            longitude: Angle::new::<degree>(normalize_longitude(longitude_deg)),
            altitude: Length::new::<kilometer>(altitude_km),
        }
        .try_into()
    }

    /// Returns geodetic latitude.
    pub const fn latitude(&self) -> Angle {
        self.latitude
    }

    /// Returns normalized longitude in the half-open range [-180°, 180°).
    pub const fn longitude(&self) -> Angle {
        self.longitude
    }

    /// Returns altitude above the model's reference ellipsoid.
    pub const fn altitude(&self) -> Length {
        self.altitude
    }
}

#[derive(Deserialize)]
struct GeodeticPositionWire {
    latitude: Angle,
    longitude: Angle,
    altitude: Length,
}
impl TryFrom<GeodeticPositionWire> for GeodeticPosition {
    type Error = PositionError;
    fn try_from(wire: GeodeticPositionWire) -> Result<Self, Self::Error> {
        use std::f64::consts::{FRAC_PI_2, PI, TAU};
        if !wire.latitude.value.is_finite()
            || !wire.longitude.value.is_finite()
            || !wire.altitude.value.is_finite()
        {
            return Err(PositionError::NonFinite);
        }
        if !(-FRAC_PI_2..=FRAC_PI_2).contains(&wire.latitude.value) {
            return Err(PositionError::LatitudeOutOfRange(
                wire.latitude.get::<degree>(),
            ));
        }
        let mut longitude = wire.longitude;
        if !(-PI..PI).contains(&longitude.value) {
            longitude = Angle::new::<radian>((longitude.value + PI).rem_euclid(TAU) - PI);
        }
        Ok(Self {
            latitude: wire.latitude,
            longitude,
            altitude: wire.altitude,
        })
    }
}

fn normalize_longitude(longitude_deg: f64) -> f64 {
    (longitude_deg + 180.0).rem_euclid(360.0) - 180.0
}

/// Invalid geodetic position input.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum PositionError {
    /// At least one scalar was NaN or infinite.
    #[error("geodetic position values must be finite")]
    NonFinite,
    /// Latitude was outside [-90°, 90°].
    #[error("latitude {0}° is outside [-90°, 90°]")]
    LatitudeOutOfRange(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_longitude() {
        let position = GeodeticPosition::from_degrees_kilometers(30.0, 540.0, 300.0).unwrap();
        assert!((position.longitude().get::<degree>() + 180.0).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_invalid_latitude() {
        assert_eq!(
            GeodeticPosition::from_degrees_kilometers(91.0, 0.0, 0.0),
            Err(PositionError::LatitudeOutOfRange(91.0))
        );
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;
    #[test]
    fn preserves_base_units_and_boundary_latitudes() {
        for latitude in [-90.0, 0.0, 90.0] {
            let position =
                GeodeticPosition::from_degrees_kilometers(latitude, 120.0, -0.5).unwrap();
            let json = serde_json::to_value(position).unwrap();
            assert_eq!(json["altitude"], -500.0);
            assert_eq!(
                serde_json::from_value::<GeodeticPosition>(json).unwrap(),
                position
            );
        }
    }
    #[test]
    fn serde_rejects_latitude_and_normalizes_longitude() {
        use std::f64::consts::PI;
        assert!(
            serde_json::from_value::<GeodeticPosition>(
                serde_json::json!({"latitude": PI, "longitude": 0.0, "altitude": 0.0})
            )
            .is_err()
        );
        let position: GeodeticPosition = serde_json::from_value(
            serde_json::json!({"latitude": 0.0, "longitude": 3.0*PI, "altitude": -10.0}),
        )
        .unwrap();
        assert!((position.longitude.value + PI).abs() < 1e-14);
    }
    #[test]
    fn wire_rejects_nonfinite_and_constructor_overflow() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for field in 0..3 {
                let wire = GeodeticPositionWire {
                    latitude: Angle::new::<radian>(if field == 0 { value } else { 0.0 }),
                    longitude: Angle::new::<radian>(if field == 1 { value } else { 0.0 }),
                    altitude: Length::new::<kilometer>(if field == 2 { value } else { 0.0 }),
                };
                assert_eq!(
                    GeodeticPosition::try_from(wire),
                    Err(PositionError::NonFinite)
                );
            }
        }
        assert_eq!(
            GeodeticPosition::from_degrees_kilometers(0.0, 0.0, f64::MAX),
            Err(PositionError::NonFinite)
        );
    }
    #[test]
    fn nested_query_point_cannot_bypass_position_validation() {
        let point = crate::QueryPoint {
            epoch: crate::Epoch::from_gregorian_utc_at_midnight(2020, 1, 1),
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
        };
        let mut json = serde_json::to_value(point).unwrap();
        json["position"]["latitude"] = serde_json::json!(2.0);
        assert!(serde_json::from_value::<crate::QueryPoint>(json).is_err());
    }
}
