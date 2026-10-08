pub(crate) use crate::SyncPolicy;
use crate::{
    IndexDataset, IndexError, SyncReport,
    database::{YearDatabase, open_existing_year, open_year},
    download::{
        IntervalFailure, IntervalFailureKind, PreparedYearDownloadReport,
        download_dataset_interval_prepared,
    },
    import::{ImportSummary, import_parsed_file},
};
use ionoray_store::{CheckMode, MaintenanceSummary, ScopedStore, StoreRoot, StoreScope};
use std::time::Instant;

pub(crate) use crate::maintenance_verify::{verify_year, year_is_ready};

pub(crate) async fn sync_year(
    store: &StoreRoot,
    year: u16,
    policy: SyncPolicy,
) -> Result<SyncReport, IndexError> {
    sync_selected(store, year, policy, &datasets()).await
}
async fn sync_selected(
    store: &StoreRoot,
    year: u16,
    policy: SyncPolicy,
    selected: &[IndexDataset],
) -> Result<SyncReport, IndexError> {
    let start = crate::parse::epoch_ms(year, 1, 1, 0)
        .map_err(|_| IndexError::InvalidNumber("year start"))?;
    let end = crate::parse::epoch_ms(
        year.checked_add(1)
            .ok_or(IndexError::InvalidNumber("year end"))?,
        1,
        1,
        0,
    )
    .map_err(|_| IndexError::InvalidNumber("year end"))?;
    let mut report = empty_report(year, policy);
    let mut failure = None;
    for dataset in selected {
        match sync_dataset_interval(store, *dataset, start, end, policy).await {
            Ok(outcome) => {
                report.records_imported += outcome.report.records_imported;
                report.cache_files_used += outcome.report.cache_files_used;
                add_summary(&mut report.summary, &outcome.report.summary);
                report.changes.add(outcome.report.changes);
                if !outcome.failures.is_empty() {
                    failure = Some(IndexError::Validation {
                        dataset: *dataset,
                        path: store.layout().root().to_owned(),
                        reason: outcome
                            .failures
                            .iter()
                            .map(|item| item.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    });
                }
            }
            Err(error) => failure = Some(error),
        }
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(report)
}

pub(crate) struct IntervalSyncOutcome {
    pub(crate) report: SyncReport,
    pub(crate) failures: Vec<IntervalFailure>,
    pub(crate) files: Vec<crate::ValidatedIndexFile>,
}

/// One dataset writer is held through recovery, transfer, import, and cleanup.
pub(crate) async fn sync_dataset_interval(
    store: &StoreRoot,
    dataset: IndexDataset,
    start: i64,
    end: i64,
    policy: SyncPolicy,
) -> Result<IntervalSyncOutcome, IndexError> {
    let started = Instant::now();
    let year = u16::try_from(crate::time::epoch_from_millis(start).to_gregorian_utc().0)
        .map_err(|_| IndexError::InvalidNumber("range year"))?;
    let scoped = scoped(store, dataset).await?;
    let guard = scoped.try_acquire_sync_lock()?;
    let mut report = empty_report(year, policy);
    let run = scoped
        .begin_maintenance(
            "indices.sync_range",
            match policy {
                SyncPolicy::Offline => "offline",
                SyncPolicy::AlwaysCheck => "refresh",
                SyncPolicy::ForceDownload => "force",
            },
            Some(year),
        )
        .await?;
    report.run_id = run.run_id.clone();
    let mut failures = Vec::new();
    let (local, local_failures, cache_count, mut files) = crate::local_recovery::recover(
        store,
        &scoped,
        dataset,
        start,
        end,
        policy == SyncPolicy::Offline,
    )
    .await?;
    report.cache_files_used += cache_count;
    add_import_report(&mut report, local);
    // Failed local objects can be recovered by an online content request.
    if policy == SyncPolicy::Offline {
        failures.extend(local_failures);
    } else {
        let mut outcome = IntervalSyncOutcome {
            report,
            failures,
            files,
        };
        import_remote(store, &scoped, dataset, start, end, policy, &mut outcome).await?;
        report = outcome.report;
        failures = outcome.failures;
        files = outcome.files;
    }
    cleanup_after_commit(store, &scoped, dataset, year, &guard, &mut failures).await;
    report.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        ?dataset,
        year,
        records_imported = report.records_imported,
        releases_created = report.summary.releases_created,
        failures = failures.len(),
        "index interval committed"
    );
    if failures.is_empty() {
        scoped.finish_maintenance(&run, report.summary).await?;
    } else {
        scoped
            .fail_maintenance(
                &run,
                "partial",
                &failures
                    .iter()
                    .map(|item| item.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
            .await?;
    }
    Ok(IntervalSyncOutcome {
        report,
        failures,
        files,
    })
}

async fn import_remote(
    store: &StoreRoot,
    scoped: &ScopedStore,
    dataset: IndexDataset,
    start: i64,
    end: i64,
    policy: SyncPolicy,
    outcome: &mut IntervalSyncOutcome,
) -> Result<(), IndexError> {
    let IntervalSyncOutcome {
        report,
        failures,
        files,
    } = outcome;
    let mode = if policy == SyncPolicy::ForceDownload {
        CheckMode::ForceContent
    } else {
        CheckMode::Metadata
    };
    let downloaded = download_dataset_interval_prepared(store, dataset, start, end, mode).await?;
    failures.extend(downloaded.failures);
    let prepared = PreparedYearDownloadReport {
        year: report.year,
        files: downloaded.files,
    };
    let mut sources: std::collections::HashMap<String, (bool, bool)> =
        std::collections::HashMap::new();
    add_summary(
        &mut report.summary,
        &crate::sync_summary::summarize_downloads(&prepared.report()),
    );
    report.cache_files_used += prepared.report().cache_file_count();
    for item in prepared.files {
        let source = sources
            .entry(item.file.source_id.clone())
            .or_insert((false, true));
        let result = async {
            let database = open_dataset(store, dataset, item.file.year, false).await?;
            import_parsed_file(&database, &item.file, item.parsed).await
        }
        .await;
        match result {
            Ok(imported) => {
                source.0 |= imported.historical_revision;
                add_import_report(report, imported);
                files.push(item.file.clone());
            }
            Err(error) => {
                source.1 = false;
                failures.push(IntervalFailure::new(
                    item.file.year,
                    item.file.month,
                    IntervalFailureKind::Parse,
                    error.to_string(),
                ));
            }
        }
    }
    for (source, (revision, complete)) in sources {
        if complete
            && !failures.iter().any(|failure| {
                failure.source_id.as_deref() == Some(source.as_str())
                    || (failure.source_id.is_none() && failure.month.is_none())
            })
        {
            scoped.finalize_acceptance(&source, revision).await?;
        }
    }
    Ok(())
}

async fn cleanup_after_commit(
    store: &StoreRoot,
    scoped: &ScopedStore,
    dataset: IndexDataset,
    year: u16,
    guard: &ionoray_store::SyncGuard,
    failures: &mut Vec<IntervalFailure>,
) {
    match crate::cleanup::retain_snapshots(store, scoped, dataset, guard).await {
        Ok(retained) => {
            if let Err(error) = scoped.prune(&retained, guard).await {
                failures.push(IntervalFailure::new(
                    year,
                    None,
                    IntervalFailureKind::Parse,
                    format!("cleanup failed after data commit: {error}"),
                ));
            }
        }
        Err(error) => failures.push(IntervalFailure::new(
            year,
            None,
            IntervalFailureKind::Parse,
            format!("retention failed after data commit: {error}"),
        )),
    }
    if let Err(error) = scoped.repair_views().await {
        failures.push(IntervalFailure::new(
            year,
            None,
            IntervalFailureKind::Parse,
            format!("browse repair pending: {error}"),
        ));
    }
}

pub(crate) async fn open_dataset(
    store: &StoreRoot,
    dataset: IndexDataset,
    year: u16,
    existing: bool,
) -> Result<YearDatabase, IndexError> {
    let path = store
        .layout()
        .index_database(dataset.provider(), dataset.as_str(), year);
    if existing {
        open_existing_year(path, dataset, year).await
    } else {
        open_year(path, dataset, year).await
    }
}
pub(crate) async fn scoped(
    store: &StoreRoot,
    dataset: IndexDataset,
) -> Result<ScopedStore, IndexError> {
    Ok(store.scoped(StoreScope::new(dataset.scope_name())?).await?)
}
pub(crate) const fn datasets() -> [IndexDataset; 5] {
    [
        IndexDataset::KpApF107,
        IndexDataset::Dst,
        IndexDataset::Ae,
        IndexDataset::IriIgRz,
        IndexDataset::IriApF107,
    ]
}
pub(crate) const fn required_partitions(dataset: IndexDataset) -> usize {
    match dataset {
        IndexDataset::Dst | IndexDataset::Ae => 12,
        _ => 1,
    }
}
fn empty_report(year: u16, policy: SyncPolicy) -> SyncReport {
    SyncReport {
        run_id: uuid::Uuid::now_v7().to_string(),
        year,
        policy,
        summary: MaintenanceSummary::default(),
        changes: crate::ChangeSummary::default(),
        records_imported: 0,
        cache_files_used: 0,
        elapsed_ms: 0,
    }
}
fn add_import_report(report: &mut SyncReport, imported: ImportSummary) {
    report.records_imported += imported.records;
    report.summary.releases_created += imported.releases_created;
    report.summary.releases_reused += imported.releases_reused;
    report.changes.add(crate::ChangeSummary {
        initial: imported.initial,
        append: imported.append,
        backfill: imported.backfill,
        revision: imported.revision,
    });
}
fn add_summary(total: &mut MaintenanceSummary, part: &MaintenanceSummary) {
    total.sources_queried += part.sources_queried;
    total.metadata_unchanged += part.metadata_unchanged;
    total.bodies_downloaded += part.bodies_downloaded;
    total.bytes_downloaded += part.bytes_downloaded;
    total.artifacts_created += part.artifacts_created;
    total.releases_created += part.releases_created;
    total.releases_reused += part.releases_reused;
}
