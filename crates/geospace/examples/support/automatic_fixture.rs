//! Fixed query into the checked-in 2026-07-16 snapshot; no user home or HTTP.
use ionoray_geospace::{Epoch, GeodeticPosition, QueryPoint};

pub fn query() -> QueryPoint {
    QueryPoint {
        epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
        position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
    }
}

// Independently transcribed from GFZ rows 2020-06-29 through 2020-07-01,
// starting at July 1 12:00 UTC and stepping backwards in three-hour bins.
pub const AP_HISTORY: [f64; 20] = [
    2.0, 4.0, 3.0, 4.0, 3.0, 5.0, 0.0, 3.0, 2.0, 2.0, 6.0, 6.0, 6.0, 2.0, 2.0, 3.0, 2.0, 2.0, 0.0,
    3.0,
];
