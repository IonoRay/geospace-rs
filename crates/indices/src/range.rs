//! Public contracts for bounded index synchronization.

use ionoray_core::Epoch;
use serde::{Deserialize, Serialize};

use crate::{IndexDataset, IndexError, time::epoch_millis};

/// Network behavior for a bounded synchronization request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Use only accepted local objects and packaged local snapshots.
    Offline,
    /// Contact upstream only when the requested local coverage is incomplete.
    Ensure,
    /// Recheck configured upstream sources and fill requested local gaps.
    #[default]
    Refresh,
}

/// One physical field whose usable coverage is required.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexField {
    /// Planetary three-hour Kp.
    Kp,
    /// Planetary three-hour ap.
    Ap3h,
    /// Daily planetary Ap.
    DailyAp,
    /// Observed daily F10.7.
    F107,
    /// Centered F10.7 average.
    F107a,
    /// Hourly Dst.
    Dst,
    /// Hourly AE.
    Ae,
    /// Daily IRI IG12.
    Ig12,
    /// Daily IRI Rz12.
    Rz12,
    /// Daily IRI adjusted F10.7.
    IriF107,
    /// IRI 81-day adjusted F10.7 average.
    IriF107a81,
    /// IRI 365-day adjusted F10.7 average.
    IriF107a365,
}

impl IndexField {
    /// Every field physically published by a dataset.
    pub const fn for_dataset(dataset: IndexDataset) -> &'static [Self] {
        match dataset {
            IndexDataset::KpApF107 => {
                &[Self::Kp, Self::Ap3h, Self::DailyAp, Self::F107, Self::F107a]
            }
            IndexDataset::Dst => &[Self::Dst],
            IndexDataset::Ae => &[Self::Ae],
            IndexDataset::IriIgRz => &[Self::Ig12, Self::Rz12],
            IndexDataset::IriApF107 => &[Self::IriF107, Self::IriF107a81, Self::IriF107a365],
        }
    }

    /// Dataset that physically publishes this field.
    pub const fn dataset(self) -> IndexDataset {
        match self {
            Self::Kp | Self::Ap3h | Self::DailyAp | Self::F107 | Self::F107a => {
                IndexDataset::KpApF107
            }
            Self::Dst => IndexDataset::Dst,
            Self::Ae => IndexDataset::Ae,
            Self::Ig12 | Self::Rz12 => IndexDataset::IriIgRz,
            Self::IriF107 | Self::IriF107a81 | Self::IriF107a365 => IndexDataset::IriApF107,
        }
    }
}

/// A UTC, left-closed/right-open request for fields in one dataset.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeRequest {
    /// Dataset owning every requested field.
    pub dataset: IndexDataset,
    /// Required physical fields.
    pub fields: Vec<IndexField>,
    /// Inclusive UTC start.
    pub start: Epoch,
    /// Exclusive UTC end.
    pub end: Epoch,
    /// Requested network behavior.
    pub mode: SyncMode,
    /// Redownload bodies before comparison. Invalid with [`SyncMode::Offline`].
    pub force: bool,
}

impl RangeRequest {
    /// Validates ownership, interval order, and incompatible network flags.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] when bounds, field ownership, or mode flags are invalid.
    pub fn validate(&self) -> Result<(), IndexError> {
        let start = epoch_millis(self.start)?;
        let end = epoch_millis(self.end)?;
        if start >= end {
            return Err(invalid_request("range must be non-empty and ordered"));
        }
        if self.fields.is_empty() {
            return Err(invalid_request("range requires at least one field"));
        }
        if self
            .fields
            .iter()
            .any(|field| field.dataset() != self.dataset)
        {
            return Err(invalid_request(
                "requested field does not belong to dataset",
            ));
        }
        if self.force && self.mode == SyncMode::Offline {
            return Err(invalid_request("force cannot be used in offline mode"));
        }
        Ok(())
    }

    pub(crate) fn bounds_ms(&self) -> Result<(i64, i64), IndexError> {
        self.validate()?;
        Ok((epoch_millis(self.start)?, epoch_millis(self.end)?))
    }
}

/// Why a requested field interval remains unavailable after a synchronization.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageGapReason {
    /// No local active value covered the range.
    LocalMissing,
    /// Offline mode did not query upstream.
    OfflineUnverified,
    /// Upstream explicitly has no source for the partition.
    RemoteUnavailable,
    /// A network request could not be completed.
    NetworkFailure,
    /// Upstream was checked but does not supply a usable value.
    UpstreamMissing,
    /// The dataset does not support the requested range.
    Unsupported,
    /// Source bytes could not satisfy the parser contract.
    ParseFailure,
}

/// One unavailable left-closed/right-open field interval, in Unix milliseconds.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CoverageGap {
    /// Field with an unavailable range.
    pub field: IndexField,
    /// Inclusive UTC start, Unix milliseconds.
    pub start_utc_ms: i64,
    /// Exclusive UTC end, Unix milliseconds.
    pub end_utc_ms: i64,
    /// Typed cause of the gap.
    pub reason: CoverageGapReason,
}

/// Actual usable coverage found for one requested field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FieldCoverage {
    /// Audited field.
    pub field: IndexField,
    /// Requested inclusive UTC start.
    pub requested_start_utc_ms: i64,
    /// Requested exclusive UTC end.
    pub requested_end_utc_ms: i64,
    /// Active non-null records found in the range.
    pub available_samples: usize,
    /// Missing subranges.
    pub gaps: Vec<CoverageGap>,
}

/// Outcome of one local-recovery or remote-source attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceAttempt {
    /// Source partition year.
    pub year: u16,
    /// Source partition month when applicable.
    pub month: Option<u8>,
    /// Whether the attempt used only local recovery.
    pub local_recovery: bool,
    /// Typed residual-gap reason for a failed attempt.
    pub gap_reason: Option<CoverageGapReason>,
    /// Stable upstream source identity, when the attempt reached a candidate.
    pub source_id: Option<String>,
    /// Canonical candidate URL, when the attempt reached a candidate.
    pub canonical_url: Option<String>,
    /// Publication edition selected for the candidate.
    pub edition: Option<crate::IndexEdition>,
    /// SHA-256 of the accepted artifact, when a body was retained.
    pub artifact_sha256: Option<String>,
    /// Physical origin of an accepted artifact.
    pub origin: Option<crate::IndexFileOrigin>,
    /// Whether the artifact was committed as a domain-accepted baseline.
    pub committed: bool,
    /// Human-readable operational outcome.
    pub outcome: String,
}

/// Whether the bounded request is fully usable by its caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeSyncStatus {
    /// Requested fields are usable and any requested source check succeeded.
    Complete,
    /// Coverage has a gap or a requested source check failed.
    Partial,
}

/// Outcome of checking upstream sources separately from local field coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCheckStatus {
    /// Offline mode or complete local coverage under Ensure skipped upstream.
    NotRequested,
    /// Requested upstream checks completed without a reported failure.
    Complete,
    /// At least one requested upstream check failed.
    Failed,
}

/// Explicit result of a bounded synchronization, including residual gaps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RangeSyncReport {
    /// Dataset synchronized.
    pub dataset: IndexDataset,
    /// Applied network mode.
    pub mode: SyncMode,
    /// Whether content downloads were forced.
    pub force: bool,
    /// Requested inclusive UTC start.
    pub requested_start_utc_ms: i64,
    /// Requested exclusive UTC end.
    pub requested_end_utc_ms: i64,
    /// Actual coverage per field.
    pub coverage: Vec<FieldCoverage>,
    /// Flattened residual gaps.
    pub gaps: Vec<CoverageGap>,
    /// Local and remote source attempts.
    pub attempts: Vec<SourceAttempt>,
    /// Whether every requested field is locally usable, regardless of refresh.
    pub coverage_status: RangeSyncStatus,
    /// Whether requested upstream checks succeeded, regardless of local coverage.
    pub source_check_status: SourceCheckStatus,
    /// Strict combined result: complete only if coverage and source checks succeeded.
    pub status: RangeSyncStatus,
    /// Parsed records imported in this operation.
    pub records_imported: usize,
    /// Physical source-check and body-transfer counters.
    pub download_summary: ionoray_store::MaintenanceSummary,
    /// Committed semantic changes by classification.
    pub changes: crate::ChangeSummary,
}

impl RangeSyncReport {
    /// Compact human-readable reason; the structured report retains every attempt.
    pub fn diagnostic(&self) -> String {
        let failures = self
            .attempts
            .iter()
            .filter(|attempt| !attempt.local_recovery && attempt.gap_reason.is_some())
            .collect::<Vec<_>>();
        let mut message = format!(
            "{:?}: {} coverage gaps; source check {:?}",
            self.dataset,
            self.gaps.len(),
            self.source_check_status
        );
        if let Some((first, reason)) = failures
            .first()
            .and_then(|first| first.gap_reason.map(|reason| (*first, reason)))
        {
            use std::fmt::Write;
            let _ = write!(
                message,
                "; {} source failure(s), first: {:?} {}",
                failures.len(),
                reason,
                first.outcome
            );
        } else if let Some(gap) = self.gaps.first() {
            use std::fmt::Write;
            let _ = write!(message, "; first gap: {:?} {:?}", gap.field, gap.reason);
        }
        message
    }
}

fn invalid_request(reason: &'static str) -> IndexError {
    IndexError::Validation {
        dataset: IndexDataset::KpApF107,
        path: std::path::PathBuf::new(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_force_in_offline_mode() {
        let request = RangeRequest {
            dataset: IndexDataset::Dst,
            fields: vec![IndexField::Dst],
            start: Epoch::from_unix_seconds(0.0),
            end: Epoch::from_unix_seconds(1.0),
            mode: SyncMode::Offline,
            force: true,
        };
        assert!(request.validate().is_err());
    }
}
