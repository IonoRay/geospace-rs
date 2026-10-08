use std::time::Instant;

use ionoray_core::Sha256Digest;
use sha2::{Digest, Sha256};
use tracing::{debug, debug_span};

use crate::IgrfError;

pub(crate) const MAX_DEGREE: usize = 13;
const EPOCH_COUNT: usize = 26;
const EXPECTED_ROWS: usize = MAX_DEGREE * (MAX_DEGREE + 2);
const COEFFICIENT_SOURCE: &str = include_str!(concat!(env!("OUT_DIR"), "/igrf14coeffs.txt"));
const COEFFICIENT_SHA256: &str = "8f8d88403028fc4ee92c4f38d97b46e0a87e2cfc496045b43c9e26c1d6b0903c";

fn verify_digest(source: &str) -> Result<Sha256Digest, IgrfError> {
    let digest = Sha256Digest::from_bytes(Sha256::digest(source).into());
    if digest.to_hex() != COEFFICIENT_SHA256 {
        return Err(IgrfError::CoefficientDigestMismatch {
            expected: COEFFICIENT_SHA256.to_owned(),
            actual: digest.to_hex(),
        });
    }
    Ok(digest)
}

#[derive(Clone, Debug)]
pub(crate) struct GaussCoefficients {
    pub(crate) g: [[f64; MAX_DEGREE + 1]; MAX_DEGREE + 1],
    pub(crate) h: [[f64; MAX_DEGREE + 1]; MAX_DEGREE + 1],
}

impl Default for GaussCoefficients {
    fn default() -> Self {
        Self {
            g: [[0.0; MAX_DEGREE + 1]; MAX_DEGREE + 1],
            h: [[0.0; MAX_DEGREE + 1]; MAX_DEGREE + 1],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoefficientSet {
    epochs: Vec<GaussCoefficients>,
    secular_variation: GaussCoefficients,
    pub(crate) digest: Sha256Digest,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ResolutionMode {
    Interpolated { lower_year: i32, upper_year: i32 },
    SecularVariation { base_year: i32 },
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedCoefficients {
    pub(crate) values: GaussCoefficients,
    pub(crate) max_degree: usize,
    pub(crate) mode: ResolutionMode,
}

impl CoefficientSet {
    pub(crate) fn load() -> Result<Self, IgrfError> {
        let started = Instant::now();
        let span = debug_span!(
            "coefficients.load",
            model = "igrf14",
            pid = std::process::id(),
            source_bytes = COEFFICIENT_SOURCE.len(),
            elapsed_us = tracing::field::Empty,
        );
        let _entered = span.enter();
        let digest = verify_digest(COEFFICIENT_SOURCE)?;
        let mut epochs = vec![GaussCoefficients::default(); EPOCH_COUNT];
        let mut secular_variation = GaussCoefficients::default();
        let mut parsed = 0;

        for (offset, row) in COEFFICIENT_SOURCE.lines().enumerate() {
            let line = offset + 1;
            let fields: Vec<_> = row.split_whitespace().collect();
            if !matches!(fields.first(), Some(&"g" | &"h")) {
                continue;
            }
            parse_row(line, &fields, &mut epochs, &mut secular_variation)?;
            parsed += 1;
        }

        if parsed != EXPECTED_ROWS {
            return Err(IgrfError::IncompleteCoefficients {
                parsed,
                expected: EXPECTED_ROWS,
            });
        }

        let elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        span.record("elapsed_us", elapsed_us);
        debug!(
            event = "igrf.coefficients.loaded",
            coefficient_rows = parsed,
            sha256 = %digest,
            elapsed_us,
            "loaded embedded IGRF coefficients"
        );
        Ok(Self {
            epochs,
            secular_variation,
            digest,
        })
    }

    pub(crate) fn resolve(&self, decimal_year: f64) -> ResolvedCoefficients {
        let max_degree = if decimal_year < 1995.0 { 10 } else { 13 };
        if decimal_year >= 2025.0 {
            return ResolvedCoefficients {
                values: combine(
                    &self.epochs[EPOCH_COUNT - 1],
                    &self.secular_variation,
                    1.0,
                    decimal_year - 2025.0,
                ),
                max_degree,
                mode: ResolutionMode::SecularVariation { base_year: 2025 },
            };
        }

        let position = (decimal_year - 1900.0) / 5.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let lower = position.floor() as usize;
        let lower_offset = i32::try_from(lower).expect("IGRF epoch index is bounded by 25");
        let lower_year = 1900 + lower_offset * 5;
        let upper_year = lower_year + 5;
        let fraction = (decimal_year - f64::from(lower_year)) / 5.0;
        ResolvedCoefficients {
            values: combine(
                &self.epochs[lower],
                &self.epochs[lower + 1],
                1.0 - fraction,
                fraction,
            ),
            max_degree,
            mode: ResolutionMode::Interpolated {
                lower_year,
                upper_year,
            },
        }
    }
}

fn parse_row(
    line: usize,
    fields: &[&str],
    epochs: &mut [GaussCoefficients],
    secular_variation: &mut GaussCoefficients,
) -> Result<(), IgrfError> {
    let invalid = |reason: String| IgrfError::InvalidCoefficient { line, reason };
    if fields.len() != EPOCH_COUNT + 4 {
        return Err(invalid(format!(
            "found {} fields, expected {}",
            fields.len(),
            EPOCH_COUNT + 4
        )));
    }
    let degree = fields[1]
        .parse::<usize>()
        .map_err(|error| invalid(error.to_string()))?;
    let order = fields[2]
        .parse::<usize>()
        .map_err(|error| invalid(error.to_string()))?;
    if degree == 0 || degree > MAX_DEGREE || order > degree {
        return Err(invalid(format!("invalid degree/order {degree}/{order}")));
    }
    let is_g = fields[0] == "g";
    for (index, value) in fields[3..3 + EPOCH_COUNT].iter().enumerate() {
        set(
            &mut epochs[index],
            is_g,
            degree,
            order,
            value
                .parse()
                .map_err(|error: std::num::ParseFloatError| invalid(error.to_string()))?,
        );
    }
    let sv = fields[EPOCH_COUNT + 3]
        .parse()
        .map_err(|error: std::num::ParseFloatError| invalid(error.to_string()))?;
    set(secular_variation, is_g, degree, order, sv);
    Ok(())
}

fn set(model: &mut GaussCoefficients, is_g: bool, degree: usize, order: usize, value: f64) {
    if is_g {
        model.g[degree][order] = value;
    } else {
        model.h[degree][order] = value;
    }
}

fn combine(
    left: &GaussCoefficients,
    right: &GaussCoefficients,
    left_weight: f64,
    right_weight: f64,
) -> GaussCoefficients {
    let mut result = GaussCoefficients::default();
    for degree in 1..=MAX_DEGREE {
        for order in 0..=degree {
            result.g[degree][order] =
                left_weight * left.g[degree][order] + right_weight * right.g[degree][order];
            result.h[degree][order] =
                left_weight * left.h[degree][order] + right_weight * right.h[degree][order];
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_modified_coefficient_asset() {
        let modified = COEFFICIENT_SOURCE.replacen("-29350.0", "-29351.0", 1);
        assert_ne!(modified, COEFFICIENT_SOURCE);
        assert!(matches!(
            verify_digest(&modified),
            Err(IgrfError::CoefficientDigestMismatch { .. })
        ));
    }

    #[test]
    fn parses_official_igrf14_table() {
        let set = CoefficientSet::load().unwrap();
        assert_eq!(
            set.digest.to_hex(),
            "8f8d88403028fc4ee92c4f38d97b46e0a87e2cfc496045b43c9e26c1d6b0903c"
        );
        assert!((set.epochs[25].g[1][0] - (-29_350.0)).abs() < f64::EPSILON);
        assert!((set.epochs[25].h[1][1] - 4_545.5).abs() < f64::EPSILON);
        assert!((set.secular_variation.g[1][0] - 12.6).abs() < f64::EPSILON);
        assert!((set.secular_variation.h[1][1] - (-21.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn resolves_interpolation_and_secular_variation() {
        let set = CoefficientSet::load().unwrap();
        let mid = set.resolve(2022.5);
        assert!((mid.values.g[1][0] - (-29_376.705)).abs() < 1.0e-10);
        assert_eq!(mid.max_degree, 13);
        let future = set.resolve(2027.0);
        assert!((future.values.g[1][0] - (-29_324.8)).abs() < 1.0e-10);
    }
}
