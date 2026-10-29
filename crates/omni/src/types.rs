use std::{collections::BTreeMap, path::PathBuf};

use ionoray_store::ArtifactRef;
use serde::{Deserialize, Serialize};

/// Distinct upstream scientific products, independent of CDF filename versions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OmniProduct {
    /// Original high-resolution OMNI.
    Hro,
    /// High-resolution OMNI using definitive Wind plasma data.
    Hro2,
}

impl OmniProduct {
    /// Official directory/dataset component.
    pub const fn dataset(self) -> &'static str {
        match self {
            Self::Hro => "hro_1min",
            Self::Hro2 => "hro2_1min",
        }
    }
    pub(crate) fn candidates(product: Option<Self>) -> Vec<Self> {
        product.map_or_else(|| vec![Self::Hro2, Self::Hro], |p| vec![p])
    }
}

/// Metadata retained verbatim for an exported OMNI variable.
#[derive(Clone, Debug, Serialize)]
pub struct VariableMetadata {
    /// CDF variable name; frame is explicit in magnetic component names.
    pub name: String,
    /// All CDF variable attributes, represented as scalar strings.
    pub attributes: BTreeMap<String, Vec<String>>,
    /// Ground indices retain UTC; solar-wind samples use OMNI's propagated time.
    pub time_reference: &'static str,
    /// Quality interpretation; HRO2 is not an index-maturity label.
    pub quality: &'static str,
}

/// One observed scalar and its validity, without clipping or interpolation.
#[derive(Debug, Serialize)]
pub struct SampleValue {
    /// Raw numeric scalar, including finite fill values; non-finite becomes null.
    pub raw: Option<f64>,
    /// Value with only missing/non-finite samples replaced by null.
    pub value: Option<f64>,
    /// Explicit missing-value reason, if applicable.
    pub missing: Option<&'static str>,
    /// True for a finite, non-fill value outside declared VALIDMIN/VALIDMAX.
    pub outside_valid_range: bool,
}

/// One minute of data and exact source identity.
#[derive(Debug, Serialize)]
pub struct OmniRecord {
    /// UTC timestamp in RFC 3339.
    pub utc: String,
    /// Original `CDF_EPOCH` milliseconds since year zero.
    pub cdf_epoch_ms: f64,
    /// Actual selected product, including fallback.
    pub product: OmniProduct,
    /// Source cadence, currently `1min`.
    pub cadence: &'static str,
    /// UTC calendar partition, `YYYY-MM`.
    pub partition: String,
    /// Upstream filename version, e.g. `v01`.
    pub file_version: String,
    /// Original monthly filename, including upstream version.
    pub filename: String,
    /// Immutable source identity.
    pub sha256: String,
    /// Zero-based source record index.
    pub record_index: usize,
    /// Upstream variable names and observed scalar values.
    pub values: BTreeMap<String, SampleValue>,
}

/// One validated, locally accepted monthly file.
#[derive(Debug, Serialize)]
pub struct FileReport {
    /// Exact input identity, flattened into the JSON file report.
    #[serde(flatten)]
    pub identity: crate::FileIdentity,
    /// Immutable downloaded bytes and digest.
    pub artifact: ArtifactRef,
    /// Human-readable CDF symlink under the OMNI subdirectory.
    pub browse_path: Option<PathBuf>,
    /// Number of source time records, independently of variable missingness.
    pub records: usize,
    /// Exported variable metadata.
    pub variables: Vec<VariableMetadata>,
}

/// Explicit record of a product missing its requested month.
#[derive(Debug, Serialize)]
pub struct MissingInterval {
    /// UTC year.
    pub year: u16,
    /// UTC month.
    pub month: u8,
    /// Product with no file.
    pub product: OmniProduct,
    /// Why this candidate was unavailable.
    pub reason: String,
}

/// Download completion, including any fallback decisions.
#[derive(Debug, Serialize)]
pub struct DownloadReport {
    /// Validated requested interval.
    pub range: crate::OmniRange,
    /// Selected files in chronological order.
    pub files: Vec<FileReport>,
    /// Missing candidates, not HTTP/network/parse errors.
    pub unavailable: Vec<MissingInterval>,
    /// Per-month selected inputs and online changes.
    pub selections: Vec<crate::MonthSelection>,
}

/// Missingness and validity for one variable over requested records.
#[derive(Debug, Default, Serialize)]
pub struct VariableCoverage {
    /// Null sample count.
    pub missing: usize,
    /// Non-fill, finite scalars outside declared limits.
    pub outside_valid_range: usize,
}

/// Record coverage is separate from physical variable coverage.
#[derive(Debug, Serialize)]
pub struct CoverageReport {
    /// Expected UTC minute timestamps in the requested interval.
    pub expected_records: usize,
    /// Actual unique timestamps read.
    pub records: usize,
    /// Consecutive missing minute ranges [start,end), in UTC Unix milliseconds.
    pub missing_time_ranges_utc_ms: Vec<[i64; 2]>,
    /// Per-variable data gaps and validity flags.
    pub variables: BTreeMap<String, VariableCoverage>,
}

/// Offline read summary; actual records are delivered to the caller's sink.
#[derive(Debug, Serialize)]
pub struct ReadReport {
    /// Requested interval.
    pub range: crate::OmniRange,
    /// Selected file identities and units/attributes.
    pub files: Vec<FileReport>,
    /// Offline selections; never implies an upstream freshness check.
    pub selections: Vec<crate::MonthSelection>,
    /// Time-record and variable coverage.
    pub coverage: CoverageReport,
    /// Propagation convention; no additional time shift is applied.
    pub propagation: &'static str,
}
