use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use ionoray_store::{CatalogArtifact, CheckMode, Store, StoreRoot, StoreScope};

use super::{ActivityDataset, ActivityError, ActivityRange, ActivityReport, parse, source};

/// Additional activity products using the existing scoped CAS/catalogs.
pub struct ActivityStore {
    pub(super) root: StoreRoot,
}

pub(super) struct OnlineState {
    pub previous_sha256: Option<String>,
    pub change: Option<&'static str>,
    pub source_check: &'static str,
    pub browse_path: Option<std::path::PathBuf>,
}

impl ActivityStore {
    /// Opens the shared data home without creating a second root database.
    /// # Errors
    /// Returns an error for an invalid or inaccessible home.
    pub fn open(home: Option<&Path>) -> Result<Self, ActivityError> {
        Ok(Self {
            root: Store::open_root(home)?,
        })
    }

    /// Checks official sources, validates raw files and accepts revisions.
    /// Missing sources retain accepted inputs, without downgrading PCN maturity.
    /// # Errors
    /// Network/format failures do not fall back to a lower-maturity source.
    pub async fn download(
        &self,
        dataset: ActivityDataset,
        range: ActivityRange,
        force: bool,
    ) -> Result<ActivityReport, ActivityError> {
        let slots = source::slots(dataset, range)?;
        self.download_slots(dataset, range, force, slots).await
    }

    async fn download_slots(
        &self,
        dataset: ActivityDataset,
        range: ActivityRange,
        force: bool,
        slots: Vec<source::Slot>,
    ) -> Result<ActivityReport, ActivityError> {
        let started = std::time::Instant::now();
        let mut last_progress = started;
        let total_dates = slots.len();
        super::progress::start(dataset, total_dates);
        let scoped = self.root.scoped(StoreScope::new(dataset.name())?).await?;
        let _guard = scoped.try_acquire_sync_lock()?;
        let mut accepted = accepted(&scoped, dataset, range).await?;
        let mut queried = BTreeSet::new();
        let mut states = BTreeMap::new();
        let mut unavailable = Vec::new();
        let mode = if force {
            CheckMode::ForceContent
        } else {
            CheckMode::Metadata
        };
        for (index, (date, candidates)) in slots.iter().enumerate() {
            if index == 0 || last_progress.elapsed().as_secs() >= 5 {
                super::progress::date_start(dataset, *date, index, total_dates);
                last_progress = std::time::Instant::now();
            }
            for candidate in candidates {
                if queried.insert(candidate.id.clone()) {
                    let request = candidate.request(dataset)?;
                    let outcome = scoped.downloads()?.check(&request, mode).await;
                    match outcome {
                        Ok(outcome) => {
                            validate_candidate(
                                candidate,
                                &outcome.artifact,
                                dataset,
                                range,
                                accepted.get(&candidate.id),
                            )?;
                            let previous_sha256 = accepted
                                .get(&candidate.id)
                                .map(|s: &CatalogArtifact| s.artifact.digest.to_string());
                            let digest = outcome.artifact.digest.to_string();
                            let change = if previous_sha256.as_ref().is_some_and(|s| *s == digest) {
                                "unchanged"
                            } else if previous_sha256.is_some() {
                                "revised"
                            } else {
                                "first_acquired"
                            };
                            let current = scoped.accept(&request, &outcome.artifact).await?;
                            states.insert(
                                candidate.id.clone(),
                                OnlineState {
                                    previous_sha256,
                                    change: Some(change),
                                    source_check: "checked",
                                    browse_path: Some(current.browse_path),
                                },
                            );
                            accepted = self::accepted(&scoped, dataset, range).await?;
                        }
                        Err(error) if source::absent(&error) => {
                            unavailable.push(candidate.id.clone());
                            if let Some(old) = accepted.get(&candidate.id) {
                                states.insert(
                                    candidate.id.clone(),
                                    OnlineState {
                                        previous_sha256: Some(old.artifact.digest.to_string()),
                                        change: Some("unchanged"),
                                        source_check: "upstream_missing",
                                        browse_path: None,
                                    },
                                );
                            }
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                if accepted.contains_key(&candidate.id) {
                    break;
                }
            }
            let completed_dates = index + 1;
            if completed_dates == total_dates || last_progress.elapsed().as_secs() >= 5 {
                super::progress::date_end(
                    dataset,
                    *date,
                    (completed_dates, total_dates),
                    (queried.len(), unavailable.len()),
                    started.elapsed().as_secs(),
                );
                last_progress = std::time::Instant::now();
            }
        }
        read_download_report(
            dataset,
            range,
            &slots,
            &accepted,
            states,
            unavailable,
            started,
        )
    }

    /// Reads accepted native data without contacting any provider.
    /// # Errors
    /// Fails on absent/corrupt inputs, parsing errors or sink failures.
    pub async fn read(
        &self,
        dataset: ActivityDataset,
        range: ActivityRange,
        emit: impl FnMut(&super::ActivityRecord) -> Result<(), ActivityError>,
    ) -> Result<ActivityReport, ActivityError> {
        let slots = source::slots(dataset, range)?;
        let scoped = self.root.scoped(StoreScope::new(dataset.name())?).await?;
        let _guard = scoped.try_acquire_read_guard()?;
        let accepted = accepted(&scoped, dataset, range).await?;
        super::read::read_selected(
            dataset,
            range,
            &slots,
            &accepted,
            BTreeMap::new(),
            Vec::new(),
            emit,
        )
    }
}

fn read_download_report(
    dataset: ActivityDataset,
    range: ActivityRange,
    slots: &[source::Slot],
    accepted: &BTreeMap<String, ionoray_store::CatalogArtifact>,
    states: BTreeMap<String, OnlineState>,
    unavailable: Vec<String>,
    started: std::time::Instant,
) -> Result<ActivityReport, ActivityError> {
    tracing::info!(
        dataset = dataset.name(),
        phase = "read-coverage",
        "activity processing phase started"
    );
    let report =
        super::read::read_selected(dataset, range, slots, accepted, states, unavailable, |_| {
            Ok(())
        })?;
    super::progress::finish(&report, started.elapsed().as_secs());
    Ok(report)
}

fn validate_candidate(
    candidate: &source::Source,
    artifact: &ionoray_store::ArtifactRef,
    dataset: ActivityDataset,
    range: ActivityRange,
    old: Option<&CatalogArtifact>,
) -> Result<(), ActivityError> {
    let parsed = parse::parse(&artifact.path, dataset, range, candidate.edition)?;
    source::validate_partition(candidate, dataset, &parsed)?;
    if let Some(old) = old
        && old.artifact.digest != artifact.digest
    {
        if super::read::verify(old).is_ok() {
            let previous = parse::parse(&old.artifact.path, dataset, range, &old.edition)?;
            parse::validate_revision(&previous, &parsed)?;
        } else {
            tracing::warn!(source_id = %candidate.id, "old raw activity input is unavailable/corrupt; validating new source independently");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

pub(super) async fn accepted(
    scoped: &ionoray_store::ScopedStore,
    dataset: ActivityDataset,
    range: ActivityRange,
) -> Result<BTreeMap<String, CatalogArtifact>, ActivityError> {
    let years: BTreeSet<_> = range.days().into_iter().map(|(year, _, _)| year).collect();
    let mut result = BTreeMap::new();
    for year in years {
        for source in scoped
            .accepted_artifacts_for_year(source::provider(dataset), dataset.name(), year)
            .await?
        {
            result.insert(source.source_id.clone(), source);
        }
    }
    Ok(result)
}
