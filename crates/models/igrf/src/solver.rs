use crate::coefficients::{GaussCoefficients, MAX_DEGREE};

const REFERENCE_RADIUS_KM: f64 = 6_371.2;
const WGS84_A_SQUARED_KM2: f64 = 40_680_631.6;
const WGS84_B_SQUARED_KM2: f64 = 40_408_296.0;
const MAX_TERMS: usize = (MAX_DEGREE + 1) * (MAX_DEGREE + 2) / 2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FieldNanotesla {
    pub(crate) north: f64,
    pub(crate) east: f64,
    pub(crate) down: f64,
}

pub(crate) fn synthesize(
    coefficients: &GaussCoefficients,
    max_degree: usize,
    latitude_deg: f64,
    longitude_deg: f64,
    altitude_km: f64,
) -> FieldNanotesla {
    let colatitude = (90.0 - latitude_deg).to_radians();
    let longitude = longitude_deg.to_radians();
    let mut ct = colatitude.cos();
    let mut st = colatitude.sin();
    let one = WGS84_A_SQUARED_KM2 * st * st;
    let two = WGS84_B_SQUARED_KM2 * ct * ct;
    let three = one + two;
    let rho = three.sqrt();
    let radius = (altitude_km * (altitude_km + 2.0 * rho)
        + (WGS84_A_SQUARED_KM2 * one + WGS84_B_SQUARED_KM2 * two) / three)
        .sqrt();
    let cd = (altitude_km + rho) / radius;
    let sd = (WGS84_A_SQUARED_KM2 - WGS84_B_SQUARED_KM2) / rho * ct * st / radius;
    let previous_ct = ct;
    ct = ct * cd - st * sd;
    st = st * cd + previous_ct * sd;

    let ratio = REFERENCE_RADIUS_KM / radius;
    let mut rr = ratio * ratio;
    let mut p = [0.0; MAX_TERMS + 1];
    let mut q = [0.0; MAX_TERMS + 1];
    let mut cl = [0.0; MAX_DEGREE + 1];
    let mut sl = [0.0; MAX_DEGREE + 1];
    p[1] = 1.0;
    p[3] = st;
    q[1] = 0.0;
    q[3] = ct;
    cl[1] = longitude.cos();
    sl[1] = longitude.sin();

    let mut north = 0.0;
    let mut east = 0.0;
    let mut down = 0.0;
    let mut degree = 0_usize;
    let mut order = 1_usize;

    let term_count = (max_degree + 1) * (max_degree + 2) / 2;
    for k in 2..=term_count {
        if degree < order {
            order = 0;
            degree += 1;
            rr *= ratio;
        }

        let degree_f = f64::from(u32::try_from(degree).expect("IGRF degree is bounded by 13"));
        let prior_degree_f = degree_f - 1.0;
        let order_f = f64::from(u32::try_from(order).expect("IGRF order is bounded by 13"));
        if order == degree {
            if k != 3 {
                let scale = (1.0 - 0.5 / order_f).sqrt();
                let j = k - degree - 1;
                p[k] = scale * st * p[j];
                q[k] = scale * (st * q[j] + ct * p[j]);
                cl[order] = cl[order - 1] * cl[1] - sl[order - 1] * sl[1];
                sl[order] = sl[order - 1] * cl[1] + cl[order - 1] * sl[1];
            }
        } else {
            let order_squared = order_f * order_f;
            let denominator = (degree_f * degree_f - order_squared).sqrt();
            let two = (prior_degree_f * prior_degree_f - order_squared).sqrt() / denominator;
            let three = (degree_f + prior_degree_f) / denominator;
            let i = k - degree;
            let j = i - degree + 1;
            p[k] = three * ct * p[i] - two * p[j];
            q[k] = three * (ct * q[i] - st * p[i]) - two * q[j];
        }

        let g = coefficients.g[degree][order] * rr;
        if order == 0 {
            north += g * q[k];
            down -= (degree_f + 1.0) * g * p[k];
        } else {
            let h = coefficients.h[degree][order] * rr;
            let combined = g * cl[order] + h * sl[order];
            north += combined * q[k];
            down -= (degree_f + 1.0) * combined * p[k];
            let azimuthal = g * sl[order] - h * cl[order];
            if st.abs() > f64::EPSILON {
                east += azimuthal * order_f * p[k] / st;
            } else {
                east += azimuthal * q[k] * ct;
            }
        }
        order += 1;
    }

    let geocentric_north = north;
    north = north * cd + down * sd;
    down = down * cd - geocentric_north * sd;
    FieldNanotesla { north, east, down }
}

#[cfg(test)]
mod tests {
    use crate::coefficients::CoefficientSet;

    use super::*;

    #[test]
    fn synthesis_returns_finite_components_at_poles() {
        let coefficients = CoefficientSet::load().unwrap().resolve(2025.0);
        for latitude in [-90.0, 90.0] {
            let field = synthesize(&coefficients.values, 13, latitude, 0.0, 0.0);
            assert!(field.north.is_finite());
            assert!(field.east.is_finite());
            assert!(field.down.is_finite());
        }
    }
}
