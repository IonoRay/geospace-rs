//! Independent, native Rust implementation of the IGRF model family.
//!
//! The model is synchronous and deterministic. It embeds official model
//! coefficients and never accesses a database, the network, or `IONORAY_HOME`.

mod coefficients;
mod error;
mod model;
mod solver;
mod time;

pub use error::IgrfError;
pub use model::{Igrf, IgrfInput, IgrfProvenance, IgrfResult, IgrfVersion, MagneticField};
