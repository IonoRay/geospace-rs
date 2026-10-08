use std::path::Path;
#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use std::time::Instant;
#[cfg(feature = "msis")]
use std::{collections::BTreeSet, sync::Mutex};

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use ionoray_core::Epoch;
use ionoray_indices::IndexStore;
#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use ionoray_indices::{IndexDataset, IndexField, RangeRequest, RangeSyncStatus, SyncMode};

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use crate::DataPolicy;
use crate::GeospaceError;
#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
use tracing::{Instrument, debug_span};

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
pub(crate) struct StageTrace {
    span: tracing::Span,
    started: Instant,
    status: &'static str,
}

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
impl StageTrace {
    pub(crate) fn span(&self) -> tracing::Span {
        self.span.clone()
    }
    pub(crate) fn new(_name: &'static str, model: &'static str) -> Self {
        Self {
            span: debug_span!(
                "model.prepare",
                model,
                elapsed_us = tracing::field::Empty,
                status = "failed",
                sample_count = tracing::field::Empty,
                source = tracing::field::Empty
            ),
            started: Instant::now(),
            status: "failed",
        }
    }

    pub(crate) fn succeeded(&mut self, sample_count: usize, source: &'static str) {
        self.status = "succeeded";
        self.span.record("status", self.status);
        self.span.record("sample_count", sample_count);
        self.span.record("source", source);
    }
}

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
impl Drop for StageTrace {
    fn drop(&mut self) {
        self.span.record("status", self.status);
        self.span.record(
            "elapsed_us",
            u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
    }
}

/// Assembly-layer model runner backed by provenance-rich geophysical indices.
pub struct Geospace {
    indices: IndexStore,
    #[cfg(feature = "msis")]
    adjustment_warning_keys: Mutex<BTreeSet<String>>,
}

impl Geospace {
    /// Opens an explicit data home, or discovers the default when `None`.
    ///
    /// # Errors
    /// Returns [`GeospaceError`] when the store cannot be opened.
    pub async fn open(home: Option<&Path>) -> Result<Self, GeospaceError> {
        Ok(Self {
            indices: IndexStore::open(home).await?,
            #[cfg(feature = "msis")]
            adjustment_warning_keys: Mutex::new(BTreeSet::new()),
        })
    }

    /// Returns the independent index store used to prepare model inputs.
    pub const fn indices(&self) -> &IndexStore {
        &self.indices
    }

    #[cfg(feature = "msis")]
    pub(crate) fn mark_adjustment_warning(&self, key: String) -> bool {
        self.adjustment_warning_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key)
    }

    #[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
    pub(crate) async fn prepare_fields_range(
        &self,
        dataset: IndexDataset,
        fields: Vec<IndexField>,
        start: Epoch,
        end: Epoch,
        policy: DataPolicy,
    ) -> Result<(), GeospaceError> {
        let span = debug_span!(
            "dataset.prepare",
            dataset = ?dataset,
            fields = ?fields,
            start = %start,
            end = %end,
            policy = ?policy,
            elapsed_us = tracing::field::Empty,
        );
        let started = Instant::now();
        let result = async {
            let report = self
                .indices
                .sync_range(RangeRequest {
                    dataset,
                    fields,
                    start,
                    end,
                    mode: match policy {
                        DataPolicy::Ensure => SyncMode::Ensure,
                        DataPolicy::Offline => SyncMode::Offline,
                        DataPolicy::Refresh => SyncMode::Refresh,
                    },
                    force: false,
                })
                .await?;
            require_complete(report)
        }
        .instrument(span.clone())
        .await;
        span.record(
            "elapsed_us",
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        result
    }
}

#[cfg(any(feature = "iri", feature = "hwm", feature = "msis"))]
fn require_complete(report: ionoray_indices::RangeSyncReport) -> Result<(), GeospaceError> {
    if report.status == RangeSyncStatus::Complete {
        Ok(())
    } else {
        Err(GeospaceError::PartialIndices(Box::new(report)))
    }
}

#[cfg(all(test, any(feature = "iri", feature = "hwm", feature = "msis")))]
mod tests {
    use super::*;
    use ionoray_indices::{CoverageGapReason, SourceCheckStatus};

    #[tokio::test]
    async fn partial_error_keeps_full_report_and_names_failed_refresh() {
        let temporary = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(temporary.path())).await.unwrap();
        let start = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
        let mut report = gs
            .indices()
            .sync_range(RangeRequest {
                dataset: IndexDataset::KpApF107,
                fields: vec![IndexField::Ap3h],
                start,
                end: start + ionoray_core::Duration::from_milliseconds(1.0),
                mode: SyncMode::Offline,
                force: false,
            })
            .await
            .unwrap();
        assert_eq!(report.gaps.len(), 0);
        // Synthetic failed source check after real, complete local recovery.
        report.mode = SyncMode::Refresh;
        report.source_check_status = SourceCheckStatus::Failed;
        report.status = RangeSyncStatus::Partial;
        report.attempts.push(ionoray_indices::SourceAttempt {
            year: 2020,
            month: None,
            local_recovery: false,
            gap_reason: Some(CoverageGapReason::NetworkFailure),
            source_id: Some("synthetic.source".to_owned()),
            canonical_url: None,
            edition: None,
            artifact_sha256: None,
            origin: None,
            committed: false,
            outcome: "synthetic connection refused".to_owned(),
        });
        let error = require_complete(report).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("0 coverage gaps; source check Failed")
        );
        assert!(error.to_string().contains("NetworkFailure"));
        assert!(error.to_string().contains("synthetic connection refused"));
        let GeospaceError::PartialIndices(report) = error else {
            panic!("expected structured partial report")
        };
        assert_eq!(report.coverage_status, RangeSyncStatus::Complete);
        assert_eq!(
            report.attempts.last().unwrap().source_id.as_deref(),
            Some("synthetic.source")
        );
    }
}
