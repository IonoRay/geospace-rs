//! Independently usable Kp, Ap, F10.7, Dst, AE, and IRI driver data contracts.

mod cleanup;
mod coverage;
mod database;
mod download;
mod error;
mod f107;
mod history;
mod import;
mod init;
mod local_recovery;
mod maintenance;
mod maintenance_types;
mod maintenance_verify;
mod parse;
mod parse_iri;
mod query;
mod query_iri;
mod range;
mod range_sync;
mod read_trace;
mod recovery;
mod sample;
mod store;
mod sync_summary;
mod time;
mod value;

pub use download::{
    IndexDataset, IndexEdition, IndexFileOrigin, QueriedIndexFile, ValidatedIndexFile,
    YearDownloadReport, YearQueryReport,
};
pub use error::IndexError;
pub use init::IndexInitReport;
pub use maintenance_types::{
    ChangeSummary, DatasetVerifyReport, SyncPolicy, SyncReport, VerifyReport,
};
pub use query::{GeomagneticIndices, GeophysicalIndices};
pub use query_iri::{IriF107Indices, IriIndices, IriMonthlyIndices};
pub use range::{
    CoverageGap, CoverageGapReason, FieldCoverage, IndexField, RangeRequest, RangeSyncReport,
    RangeSyncStatus, SourceAttempt, SourceCheckStatus, SyncMode,
};
pub use sample::{IndexSample, QualityFlag, TimeInterval, ValueDerivation};
pub use store::{EnsureReport, IndexStore};
pub use value::{AdjustedF107, Ae, Ap, Dst, F107, Ig12, IndexValueError, Kp, Rz12};
