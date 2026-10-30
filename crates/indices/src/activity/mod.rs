//! Additional geomagnetic activity products, preserving native cadences.

mod error;
mod parse;
mod progress;
mod range;
mod read;
mod source;
mod store;
mod types;

pub use error::ActivityError;
pub use range::ActivityRange;
pub use store::ActivityStore;
pub use types::{
    ActivityCoverage, ActivityDataset, ActivityFile, ActivityRecord, ActivityReport, ActivityValue,
};
