use serde::{Deserialize, Serialize};
use thiserror::Error;
use uom::si::{
    angle::degree,
    f64::{Angle, Length},
    length::kilometer,
};

/// A WGS 84-style geodetic position.
///
/// The type preserves physical units. Datum-specific transformations remain in
/// an optional coordinate adapter rather than being implicit here.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

        Ok(Self {
            latitude: Angle::new::<degree>(latitude_deg),
            longitude: Angle::new::<degree>(normalize_longitude(longitude_deg)),
            altitude: Length::new::<kilometer>(altitude_km),
        })
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
