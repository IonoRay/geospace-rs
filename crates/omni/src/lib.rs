//! OMNI one-minute CDF data, using cdf-rs directly for decoding.

mod error;
mod parse;
mod range;
mod read;
mod selection;
mod source;
mod store;
mod types;

pub use error::OmniError;
pub use range::OmniRange;
pub use selection::{FileIdentity, MonthSelection, SelectionChange, SelectionReason, SourceCheck};
pub use store::OmniStore;
pub use types::{
    CoverageReport, DownloadReport, FileReport, MissingInterval, OmniProduct, OmniRecord,
    ReadReport, SampleValue, VariableCoverage, VariableMetadata,
};
