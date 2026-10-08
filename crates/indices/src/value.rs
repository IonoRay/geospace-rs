use serde::{Deserialize, Serialize};
use thiserror::Error;

macro_rules! finite_value {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
        pub struct $name(f64);

        impl $name {
            #[doc = concat!("Creates a validated ", $description, " value.")]
            ///
            /// # Errors
            ///
            /// Returns [`IndexValueError::NonFinite`] for NaN or infinity.
            pub fn new(value: f64) -> Result<Self, IndexValueError> {
                if value.is_finite() {
                    Ok(Self(value))
                } else {
                    Err(IndexValueError::NonFinite(stringify!($name)))
                }
            }

            #[doc = concat!("Returns the scalar ", $description, " value.")]
            pub const fn value(self) -> f64 {
                self.0
            }
        }
    };
}

finite_value!(Ap, "planetary Ap index");
finite_value!(F107, "10.7 cm solar radio flux");
finite_value!(AdjustedF107, "10.7 cm solar radio flux adjusted to 1 AU");
finite_value!(
    Ig12,
    "12-month running mean ionospheric IG index (which may be negative)"
);
finite_value!(Rz12, "12-month running mean sunspot number");
finite_value!(Dst, "Dst index in nT");
finite_value!(Ae, "AE index in nT");

/// Planetary Kp index in the conventional range `[0, 9]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kp(f64);

impl Kp {
    /// Creates a finite Kp value within the conventional range.
    ///
    /// # Errors
    ///
    /// Returns [`IndexValueError::NonFinite`] for NaN or infinity and
    /// [`IndexValueError::KpOutOfRange`] outside `[0, 9]`.
    pub fn new(value: f64) -> Result<Self, IndexValueError> {
        if !value.is_finite() {
            return Err(IndexValueError::NonFinite("Kp"));
        }
        if !(0.0..=9.0).contains(&value) {
            return Err(IndexValueError::KpOutOfRange(value));
        }
        Ok(Self(value))
    }

    /// Returns the scalar Kp value.
    pub const fn value(self) -> f64 {
        self.0
    }
}

/// Invalid scalar index value.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum IndexValueError {
    /// A source supplied NaN or infinity.
    #[error("{0} must be finite")]
    NonFinite(&'static str),
    /// Kp was outside its defined range.
    #[error("Kp {0} is outside [0, 9]")]
    KpOutOfRange(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_kp_range() {
        assert!((Kp::new(2.3).unwrap().value() - 2.3).abs() < f64::EPSILON);
        assert_eq!(Kp::new(9.1), Err(IndexValueError::KpOutOfRange(9.1)));
    }
}
