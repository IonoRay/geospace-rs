use std::{
    fs,
    path::{Path, PathBuf},
};

use ionoray_store::{CheckMode, DownloadRequest, ScopedStore, Store, StoreHome, StoreScope};
use url::Url;

use crate::{
    DownloadReport, MissingInterval, OmniError, OmniProduct, OmniRange, SourceCheck, parse,
    range::Month, read::verify, selection::selection_report, source,
};

/// OMNI data home, isolated under `<IONORAY_HOME>/omni/`.
pub struct OmniStore {
    pub(crate) scoped: ScopedStore,
    base: String,
}

impl OmniStore {
    /// Opens the OMNI child directory using the shared home resolution rules.
    /// # Errors
    /// Returns an error for an invalid home or inaccessible catalog.
    pub async fn open(home: Option<&Path>) -> Result<Self, OmniError> {
        let home = StoreHome::discover(home)?;
        Self::open_at(home.as_path(), source::BASE).await
    }

    async fn open_at(home: &Path, base: &str) -> Result<Self, OmniError> {
        let scoped =
            Store::open_scoped(Some(&home.join("omni")), StoreScope::new("omni-1min")?).await?;
        Ok(Self {
            scoped,
            base: base.into(),
        })
    }

    /// Root containing the OMNI catalogs, immutable CDFs, and browsing symlinks.
    pub fn path(&self) -> PathBuf {
        self.scoped.layout().root().to_owned()
    }

    /// Downloads intersecting months, validates CDFs, then accepts each file.
    /// `None` rechecks HRO2 on every invocation and upgrades accepted HRO when
    /// HRO2 becomes available. Missing upstream files never downgrade a local HRO2.
    /// # Errors
    /// Network, integrity, and parsing errors stop without product fallback.
    /// Months already accepted remain usable if a later month fails.
    pub async fn download(
        &self,
        range: OmniRange,
        product: Option<OmniProduct>,
        force: bool,
    ) -> Result<DownloadReport, OmniError> {
        let started = std::time::Instant::now();
        let months = range.months();
        let total_months = months.len();
        tracing::info!(
            dataset = "omni",
            total_months,
            force,
            "OMNI processing started"
        );
        let _guard = self.scoped.try_acquire_sync_lock()?;
        let mut report = DownloadReport {
            range,
            files: Vec::new(),
            unavailable: Vec::new(),
            selections: Vec::new(),
        };
        for (index, month) in months.into_iter().enumerate() {
            tracing::info!(
                dataset = "omni",
                year = month.year,
                month = month.month,
                phase = "check-validate-accept",
                completed_months = index,
                total_months,
                remaining_months = total_months - index,
                "OMNI month started"
            );
            let previous = self.select_month(month, product).await?;
            let mut downloaded = None;
            let mut source_check = SourceCheck::Checked;
            for candidate in OmniProduct::candidates(product) {
                if let Some(file) = self.download_month(candidate, month, force).await? {
                    downloaded = Some(file);
                    break;
                }
                report.unavailable.push(MissingInterval {
                    year: month.year,
                    month: month.month,
                    product: candidate,
                    reason: "official directory/file absent (404/410 or no matching monthly CDF)"
                        .into(),
                });
                if previous
                    .as_ref()
                    .is_some_and(|s| s.identity.product == candidate)
                {
                    source_check = SourceCheck::UpstreamMissing;
                    break;
                }
            }
            let selected = self
                .select_month(month, product)
                .await?
                .ok_or_else(|| missing_month(month, product))?;
            let (cdf, browse_path) = if let Some((cdf, path)) = downloaded {
                (cdf, Some(path))
            } else {
                verify(&selected.source.artifact)?;
                (
                    parse::decode(
                        &selected.source.artifact.path,
                        selected.identity.product,
                        month,
                    )?,
                    None,
                )
            };
            report.selections.push(selection_report(
                &selected.identity,
                previous.map(|s| s.identity),
                product,
                source_check,
            ));
            let file = selected.file_report(&cdf, browse_path)?;
            tracing::info!(
                dataset = "omni",
                year = month.year,
                month = month.month,
                records = file.records,
                selected_file_bytes = file.artifact.byte_size,
                completed_months = index + 1,
                total_months,
                remaining_months = total_months - index - 1,
                elapsed_secs = started.elapsed().as_secs(),
                "OMNI month finished"
            );
            report.files.push(file);
        }
        tracing::info!(
            dataset = "omni",
            selected_files = report.files.len(),
            source_records = report.files.iter().map(|f| f.records).sum::<usize>(),
            unavailable_candidates = report.unavailable.len(),
            elapsed_secs = started.elapsed().as_secs(),
            "OMNI processing finished"
        );
        Ok(report)
    }

    async fn download_month(
        &self,
        product: OmniProduct,
        month: Month,
        force: bool,
    ) -> Result<Option<(cdf::cdf::Cdf, PathBuf)>, OmniError> {
        let url = source::directory(&self.base, product, month);
        let directory = DownloadRequest::new(
            format!(
                "nasa.omni.directory.{}.{:04}",
                product.dataset(),
                month.year
            ),
            Url::parse(&url).map_err(|e| OmniError::Invalid(e.to_string()))?,
            "index.html",
        )?;
        let manager = self.scoped.downloads()?;
        let listing = match manager.download(&directory).await {
            Ok(outcome) => fs::read_to_string(outcome.artifact.path)?,
            Err(e) if source::missing(&e) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let Some(filename) = source::latest_filename(&listing, product, month)? else {
            return Ok(None);
        };
        let request = source::request(&self.base, product, month, &filename)?;
        let mode = if force {
            CheckMode::ForceContent
        } else {
            CheckMode::Metadata
        };
        let outcome = match manager.check(&request, mode).await {
            Ok(outcome) => outcome,
            Err(e) if source::missing(&e) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let cdf = parse::decode(&outcome.artifact.path, product, month)?;
        let accepted = self.scoped.accept(&request, &outcome.artifact).await?;
        Ok(Some((cdf, accepted.browse_path)))
    }
}

pub(crate) fn missing_month(month: Month, product: Option<OmniProduct>) -> OmniError {
    OmniError::MissingMonth {
        year: month.year,
        month: month.month,
        products: OmniProduct::candidates(product)
            .iter()
            .map(|p| p.dataset())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod upgrade_tests;
