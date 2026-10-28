use std::path::PathBuf;

use ionoray_core::Sha256Digest;
use ionoray_store::CatalogArtifact;
use serde::Serialize;
use url::Url;

use crate::{FileReport, OmniError, OmniProduct, OmniStore, parse, range::Month, source};

/// Exact accepted input selected for one calendar month.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileIdentity {
    /// Actual scientific product; independent of filename revisions.
    pub product: OmniProduct,
    /// Source cadence, currently always `1min`.
    pub cadence: &'static str,
    /// UTC calendar partition, `YYYY-MM`.
    pub partition: String,
    /// Upstream filename version, e.g. `v01`.
    pub file_version: String,
    /// Filename from the accepted request, not from a later failed attempt.
    pub filename: String,
    /// Immutable byte identity.
    pub sha256: Sha256Digest,
    /// Stable product/month source identifier.
    pub source_id: String,
    /// Exact accepted request URL.
    pub source_url: String,
    /// Time the immutable bytes were downloaded.
    pub downloaded_at_utc_ms: i64,
    /// Time those bytes were accepted for this source.
    pub accepted_at_utc_ms: i64,
}

/// Why the selected product is used under this invocation's policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionReason {
    /// Automatic selection of locally accepted HRO2.
    PreferredProduct,
    /// Automatic selection of HRO because no HRO2 has been accepted locally.
    FallbackProduct,
    /// The caller fixed a product explicitly.
    ExplicitProduct,
}

/// Change to a month's selected input during online synchronization.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionChange {
    /// No input was previously accepted under this policy.
    FirstAcquired,
    /// HRO was replaced as the automatic selection by HRO2.
    ProductUpgraded,
    /// Same product, changed version or SHA-256.
    ProductRevised,
    /// Product, version, and SHA-256 did not change.
    Unchanged,
}

/// Upstream-check status of the selected product; not a data-coverage assertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCheck {
    /// Selected product was successfully checked and validated online.
    Checked,
    /// Upstream lacks that file; an accepted local file was retained.
    UpstreamMissing,
    /// Offline read made no upstream request.
    OfflineNotChecked,
}

/// One invocation's month selection and any successful online transition.
#[derive(Debug, Serialize)]
pub struct MonthSelection {
    /// UTC calendar partition, `YYYY-MM`.
    pub partition: String,
    /// Selected input before online synchronization; null for offline reads.
    pub previous_selected: Option<FileIdentity>,
    /// Input this invocation actually uses.
    pub selected: FileIdentity,
    /// Selection policy outcome.
    pub selection_reason: SelectionReason,
    /// Online transition; null for offline reads, which do not infer history.
    pub change: Option<SelectionChange>,
    /// Independent upstream-check state.
    pub source_check: SourceCheck,
}

pub(crate) struct SelectedMonth {
    pub identity: FileIdentity,
    pub source: CatalogArtifact,
}

impl SelectedMonth {
    pub(crate) fn file_report(
        self,
        cdf: &cdf::cdf::Cdf,
        browse_path: Option<PathBuf>,
    ) -> Result<FileReport, OmniError> {
        let mut artifact = self.source.artifact;
        // CAS metadata keeps the first filename for identical bytes. Input
        // identity must instead use the filename of this accepted source URL.
        artifact
            .original_filename
            .clone_from(&self.identity.filename);
        Ok(FileReport {
            identity: self.identity,
            artifact,
            browse_path,
            records: parse::times(cdf)?.len(),
            variables: parse::metadata(cdf)?,
        })
    }
}

impl OmniStore {
    /// Shared resolver for online completion and offline reads. Callers hold
    /// the existing scope lock, so product selection cannot change mid-read.
    pub(crate) async fn select_month(
        &self,
        month: Month,
        product: Option<OmniProduct>,
    ) -> Result<Option<SelectedMonth>, OmniError> {
        for candidate in OmniProduct::candidates(product) {
            let sources = self
                .scoped
                .accepted_artifacts_for_year("nasa-spdf", candidate.dataset(), month.year)
                .await?;
            if let Some(source) = sources.into_iter().find(|s| s.month == Some(month.month)) {
                let url = Url::parse(&source.canonical_url)
                    .map_err(|e| OmniError::Invalid(e.to_string()))?;
                let filename = url
                    .path_segments()
                    .and_then(|mut segments| segments.next_back())
                    .ok_or_else(|| {
                        OmniError::Invalid("accepted OMNI URL has no filename".into())
                    })?;
                let identity = FileIdentity {
                    product: candidate,
                    cadence: "1min",
                    partition: partition(month),
                    file_version: source::file_version(filename, candidate, month)?,
                    filename: filename.into(),
                    sha256: source.artifact.digest,
                    source_id: source.source_id.clone(),
                    source_url: source.canonical_url.clone(),
                    downloaded_at_utc_ms: source.artifact.downloaded_at_utc_ms,
                    accepted_at_utc_ms: source.accepted_at_utc_ms.ok_or_else(|| {
                        OmniError::Invalid("accepted OMNI file lacks acceptance time".into())
                    })?,
                };
                return Ok(Some(SelectedMonth { identity, source }));
            }
        }
        Ok(None)
    }
}

pub(crate) fn selection_report(
    selected: &FileIdentity,
    previous_selected: Option<FileIdentity>,
    product: Option<OmniProduct>,
    source_check: SourceCheck,
) -> MonthSelection {
    let change = (source_check != SourceCheck::OfflineNotChecked)
        .then(|| change(previous_selected.as_ref(), selected));
    MonthSelection {
        partition: selected.partition.clone(),
        previous_selected,
        selected: selected.clone(),
        selection_reason: if product.is_some() {
            SelectionReason::ExplicitProduct
        } else if selected.product == OmniProduct::Hro2 {
            SelectionReason::PreferredProduct
        } else {
            SelectionReason::FallbackProduct
        },
        change,
        source_check,
    }
}

fn change(previous: Option<&FileIdentity>, selected: &FileIdentity) -> SelectionChange {
    match previous {
        None => SelectionChange::FirstAcquired,
        Some(old) if old.product != selected.product => SelectionChange::ProductUpgraded,
        Some(old) if old.file_version != selected.file_version || old.sha256 != selected.sha256 => {
            SelectionChange::ProductRevised
        }
        Some(_) => SelectionChange::Unchanged,
    }
}

pub(crate) fn partition(month: Month) -> String {
    format!("{:04}-{:02}", month.year, month.month)
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
