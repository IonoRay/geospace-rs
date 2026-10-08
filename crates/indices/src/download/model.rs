use ionoray_store::{DownloadOutcome, RemoteQueryOutcome};
use serde::{Deserialize, Serialize};

/// Independently published geophysical index dataset.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IndexDataset {
    /// GFZ Kp, ap, Ap, and F10.7 cumulative daily file.
    KpApF107,
    /// WDC Kyoto hourly Dst index.
    Dst,
    /// WDC Kyoto auroral electrojet index.
    Ae,
    /// IRI monthly IG12 and Rz12 driver file.
    IriIgRz,
    /// IRI daily adjusted F10.7 driver file.
    IriApF107,
}

/// Publication maturity selected for one remote file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IndexEdition {
    /// A cumulative upstream file that is expected to change.
    Rolling,
    /// WDC final values.
    Final,
    /// WDC provisional values that may later be revised.
    Provisional,
    /// WDC realtime values used only when more mature data is absent.
    Realtime,
}

/// Origin selected for one validated source file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexFileOrigin {
    /// Bytes came from the configured upstream URL.
    Upstream,
    /// Upstream access failed or was disabled and packaged bytes were used.
    BundledCache,
    /// Bytes were already present in the user's content-addressed store.
    LocalCas,
}

/// One downloaded file after format and requested-period validation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValidatedIndexFile {
    /// Stable remote source-object identity.
    pub source_id: String,
    /// Canonical upstream URL used for the observation, when one was contacted.
    pub canonical_url: Option<String>,
    /// Dataset carried by the file.
    pub dataset: IndexDataset,
    /// Requested year verified inside the file.
    pub year: u16,
    /// Month for monthly files; absent for cumulative files.
    pub month: Option<u8>,
    /// Selected upstream publication maturity.
    pub edition: IndexEdition,
    /// Number of daily or hourly records verified for the period.
    pub records: usize,
    /// Whether the selected bytes came from upstream or the packaged cache.
    pub origin: IndexFileOrigin,
    /// CAS and observation outcome.
    pub download: DownloadOutcome,
}

/// Complete validated raw-file set needed for one calendar year.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct YearDownloadReport {
    /// Requested calendar year.
    pub year: u16,
    /// Complete source-file set selected for the requested operation.
    pub files: Vec<ValidatedIndexFile>,
}

/// Metadata-only query for one selected upstream candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueriedIndexFile {
    /// Dataset carried by the source.
    pub dataset: IndexDataset,
    /// Requested year.
    pub year: u16,
    /// Optional month.
    pub month: Option<u8>,
    /// Candidate publication edition.
    pub edition: IndexEdition,
    /// Stable source-object identity.
    pub source_id: String,
    /// Persisted remote query result.
    pub query: RemoteQueryOutcome,
}

/// Metadata-only upstream query for every file required by one year.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct YearQueryReport {
    /// Queried calendar year.
    pub year: u16,
    /// One selected candidate per logical source file.
    pub files: Vec<QueriedIndexFile>,
}

impl YearDownloadReport {
    /// Total verified source files.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Total verified daily or hourly records.
    pub fn record_count(&self) -> usize {
        self.files.iter().map(|file| file.records).sum()
    }

    /// Number of files served by the packaged cache without network access.
    pub fn cache_file_count(&self) -> usize {
        self.files
            .iter()
            .filter(|file| file.origin == IndexFileOrigin::BundledCache)
            .count()
    }
}

impl IndexEdition {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rolling => "rolling",
            Self::Final => "final",
            Self::Provisional => "provisional",
            Self::Realtime => "realtime",
        }
    }

    pub(crate) const fn priority(self) -> i32 {
        match self {
            Self::Final => 300,
            Self::Provisional => 200,
            Self::Realtime | Self::Rolling => 100,
        }
    }
}

impl IndexDataset {
    /// Isolated object-catalog and CAS scope for this physical dataset.
    pub(crate) const fn scope_name(self) -> &'static str {
        match self {
            Self::KpApF107 => "kp-ap-f107",
            Self::Dst => "dst",
            Self::Ae => "ae",
            Self::IriIgRz => "iri-ig-rz",
            Self::IriApF107 => "iri-apf107",
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::KpApF107 => "kp-ap-f107",
            Self::Dst => "dst",
            Self::Ae => "ae",
            Self::IriIgRz => "ig-rz",
            Self::IriApF107 => "apf107",
        }
    }

    pub(crate) const fn provider(self) -> &'static str {
        match self {
            Self::KpApF107 => "gfz",
            Self::Dst | Self::Ae => "wdc-kyoto",
            Self::IriIgRz | Self::IriApF107 => "iri",
        }
    }
}
