use std::{collections::BTreeMap, path::PathBuf};

use ionoray_store::ArtifactRef;
use serde::{Deserialize, Serialize};

/// Independently maintained source products; cadence is part of their identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityDataset {
    /// GFZ half-hour Hp30 and ap30.
    Hp30,
    /// GFZ hourly Hp60 and ap60.
    Hp60,
    /// DTU one-minute PCN, definitive then provisional/quicklook.
    Pcn,
}

impl ActivityDataset {
    /// Stable dataset/scope name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hp30 => "hp30",
            Self::Hp60 => "hp60",
            Self::Pcn => "pcn",
        }
    }
    /// Native spacing, not resampled from another index.
    pub const fn cadence_seconds(self) -> i64 {
        match self {
            Self::Hp30 => 1800,
            Self::Hp60 => 3600,
            Self::Pcn => 60,
        }
    }
    /// Variable names returned by this dataset.
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Hp30 => &["Hp30", "ap30"],
            Self::Hp60 => &["Hp60", "ap60"],
            Self::Pcn => &["PCN"],
        }
    }
}

/// Native scalar with explicit missingness; valid negative PC values survive.
#[derive(Clone, Debug, Serialize)]
pub struct ActivityValue {
    /// Finite original scalar, including finite missing sentinels.
    pub raw: Option<f64>,
    /// Missing sentinel/non-finite values become null.
    pub value: Option<f64>,
    /// Explicit missing-value reason.
    pub missing: Option<&'static str>,
}

/// One native observation with its raw source line and immutable input identity.
#[derive(Clone, Debug, Serialize)]
pub struct ActivityRecord {
    /// Start of the native interval (UTC).
    pub utc: String,
    /// Exclusive end of that interval (UTC), not an interpolation window.
    pub end_utc: String,
    /// Native product.
    pub dataset: ActivityDataset,
    /// Explicit native spacing.
    pub cadence_seconds: i64,
    /// Source metadata classification; never inferred from the scalar.
    pub quality: String,
    /// Source header and reserved flags used by this row.
    pub source_flags: BTreeMap<String, String>,
    /// Dimensionless for Hpo/apo; mV/m for PC.
    pub units: &'static str,
    /// Values in source units without clipping, interpolation, or resampling.
    pub values: BTreeMap<String, ActivityValue>,
    /// Stable accepted source key.
    pub source_id: String,
    /// Exact immutable raw file hash.
    pub sha256: String,
    /// One-based line number in the raw source file.
    pub source_line: usize,
}

/// Selected physical input and update result.
#[derive(Debug, Serialize)]
pub struct ActivityFile {
    /// Stable source key.
    pub source_id: String,
    /// Publication maturity or rolling-source edition.
    pub edition: String,
    /// Accepted canonical URL, independent of later failed candidates.
    pub source_url: String,
    /// Exact raw bytes and content identity.
    pub artifact: ArtifactRef,
    /// CDF/ASCII browsing link when freshly accepted.
    pub browse_path: Option<PathBuf>,
    /// Previous accepted content for this physical source, if any.
    pub previous_sha256: Option<String>,
    /// `first_acquired`, revised, unchanged; null during offline reads.
    pub change: Option<&'static str>,
    /// checked, `upstream_missing`, or `offline_not_checked`.
    pub source_check: &'static str,
    /// Source header, including license and original format notes.
    pub header: Vec<String>,
}

/// Record gaps and variable missingness, reported separately.
#[derive(Debug, Serialize)]
pub struct ActivityCoverage {
    /// Native interval starts expected within the request.
    pub expected_records: usize,
    /// Distinct source timestamps present in the request.
    pub records: usize,
    /// Missing UTC start-time ranges, in Unix milliseconds.
    pub missing_time_ranges_utc_ms: Vec<[i64; 2]>,
    /// Missing scalar counts by variable among present records.
    pub missing_values: BTreeMap<String, usize>,
}

/// Download/offline-read report for a single native product.
#[derive(Debug, Serialize)]
pub struct ActivityReport {
    /// Requested native product.
    pub dataset: ActivityDataset,
    /// Requested UTC interval.
    pub range: super::ActivityRange,
    /// Exact files actually selected, without duplicate rolling files.
    pub files: Vec<ActivityFile>,
    /// Upstream candidates missing during an online check.
    pub unavailable_sources: Vec<String>,
    /// Coverage from the accepted inputs.
    pub coverage: ActivityCoverage,
}
