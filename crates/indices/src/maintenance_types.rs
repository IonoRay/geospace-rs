use crate::IndexDataset;
use ionoray_store::MaintenanceSummary;
use serde::{Deserialize, Serialize};

/// Upstream synchronization depth.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum SyncPolicy {
    /// Check metadata and transfer only potentially changed content.
    #[default]
    AlwaysCheck,
    /// Redownload every body and compare its SHA-256 digest.
    ForceDownload,
    /// Do not access the network; import all matching local CAS objects.
    Offline,
}

/// Complete result of one upstream-to-year-database synchronization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncReport {
    /// Maintenance audit identity.
    pub run_id: String,
    /// Synchronized calendar year.
    pub year: u16,
    /// Applied policy.
    pub policy: SyncPolicy,
    /// Aggregate maintenance counters.
    pub summary: MaintenanceSummary,
    /// Semantic partition changes committed by this operation.
    pub changes: ChangeSummary,
    /// Parsed records created during this run.
    pub records_imported: usize,
    /// Packaged snapshot files used without a successful remote transfer.
    pub cache_files_used: usize,
    /// Wall-clock operation duration.
    pub elapsed_ms: u64,
}

/// Counts of committed semantic changes, separate from physical downloads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeSummary {
    /// First accepted data snapshots.
    pub initial: usize,
    /// Snapshots adding only samples after previous coverage.
    pub append: usize,
    /// Snapshots filling earlier missing samples.
    pub backfill: usize,
    /// Snapshots revising existing values, missingness, or quality.
    pub revision: usize,
}

impl ChangeSummary {
    pub(crate) fn add(&mut self, part: Self) {
        self.initial += part.initial;
        self.append += part.append;
        self.backfill += part.backfill;
        self.revision += part.revision;
    }
}

/// Local integrity and coverage report for one calendar year.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VerifyReport {
    /// Verified calendar year.
    pub year: u16,
    /// Number of CAS origins checked.
    pub artifacts_verified: usize,
    /// Dataset-specific ready and required partition counts.
    pub datasets: Vec<DatasetVerifyReport>,
}

/// Verified release coverage for one independently maintained dataset.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DatasetVerifyReport {
    /// Dataset identity.
    pub dataset: IndexDataset,
    /// Ready release partitions.
    pub ready_partitions: usize,
    /// Minimum partitions required for complete yearly coverage.
    pub required_partitions: usize,
}
