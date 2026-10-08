//! Inspect offline index coverage, typed values, and provenance.
//!
//! This program uses only checked-in index snapshots in an isolated temporary
//! home. It deliberately does not initialize process tracing: diagnostics are
//! optional application setup, not part of the default data path.

use ionoray_geospace::{Duration, Epoch};
use ionoray_indices::{
    CoverageGapReason, IndexDataset, IndexField, IndexSample, IndexStore, RangeRequest,
    RangeSyncReport, RangeSyncStatus, SyncMode,
};
use serde::Serialize;

const EPOCH: &str = "2020-07-01T12:00:00 UTC";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    eprintln!("isolated temporary data home: {}", home.path().display());
    eprintln!("runtime policy: Offline; HTTP disabled; home removed on normal exit");
    let indices = IndexStore::open(Some(home.path())).await?;
    let epoch = EPOCH.parse::<Epoch>()?;

    // Breakpoint 1: inspect this bounded Offline request before local recovery.
    let range = request(
        IndexDataset::KpApF107,
        vec![IndexField::Ap3h, IndexField::F107],
        epoch,
    );
    let range_report = indices.sync_range(range).await?;
    print_report("space_weather", &range_report)?;
    require_complete(&range_report)?;

    let monthly_report = indices
        .sync_range(request(
            IndexDataset::IriIgRz,
            vec![IndexField::Ig12, IndexField::Rz12],
            epoch,
        ))
        .await?;
    print_report("iri_monthly", &monthly_report)?;
    require_complete(&monthly_report)?;
    let iri_report = indices
        .sync_range(request(
            IndexDataset::IriApF107,
            vec![IndexField::IriF107, IndexField::IriF107a81],
            epoch,
        ))
        .await?;
    print_report("iri_flux", &iri_report)?;
    require_complete(&iri_report)?;

    // Breakpoint 2: each typed result carries its own quality and provenance.
    let ap = indices.ap_at(epoch).await?;
    let f107 = indices.f107_at(epoch).await?;
    let monthly = indices.iri_monthly_at(epoch).await?;
    let iri_f107 = indices.iri_f107_at(epoch).await?;
    print_sample("ap_3h", &ap)?;
    print_sample("f107_observed", &f107)?;
    print_sample("iri_ig12", &monthly.ig12)?;
    print_sample("iri_rz12", &monthly.rz12)?;
    print_sample("iri_f107_daily", &iri_f107.daily)?;
    print_sample("iri_f107_81_day", &iri_f107.average_81_day)?;

    // Breakpoint 3: no Dst fixture exists, so Offline reports a typed gap and
    // has made zero HTTP source queries.
    let missing = indices
        .sync_range(request(IndexDataset::Dst, vec![IndexField::Dst], epoch))
        .await?;
    assert_eq!(missing.status, RangeSyncStatus::Partial);
    assert_eq!(missing.download_summary.sources_queried, 0);
    assert!(!missing.gaps.is_empty());
    assert!(
        missing
            .gaps
            .iter()
            .all(|gap| gap.reason == CoverageGapReason::OfflineUnverified)
    );
    print_report("dst_missing_offline", &missing)?;
    Ok(())
}

fn request(dataset: IndexDataset, fields: Vec<IndexField>, start: Epoch) -> RangeRequest {
    RangeRequest {
        dataset,
        fields,
        start,
        end: start + Duration::from_milliseconds(1.0),
        mode: SyncMode::Offline,
        force: false,
    }
}

fn require_complete(report: &RangeSyncReport) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(report.download_summary.sources_queried, 0);
    if report.status == RangeSyncStatus::Complete {
        Ok(())
    } else {
        Err(report.diagnostic().into())
    }
}

fn print_report(name: &str, report: &RangeSyncReport) -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "kind": "range",
            "name": name,
            "dataset": report.dataset,
            "start_utc_ms": report.requested_start_utc_ms,
            "end_utc_ms": report.requested_end_utc_ms,
            "status": report.status,
            "coverage_status": report.coverage_status,
            "source_check_status": report.source_check_status,
            "records_imported": report.records_imported,
            "source_queries": report.download_summary.sources_queried,
            "gaps": report.gaps,
            "attempts": report.attempts,
        }))?
    );
    Ok(())
}

fn print_sample<T: Serialize>(
    name: &str,
    sample: &IndexSample<T>,
) -> Result<(), serde_json::Error> {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "kind": "sample",
            "name": name,
            "value": sample.value,
            "interval": sample.interval,
            "quality": sample.quality,
            "derivation": sample.derivation,
            "source": {
                "release_id": sample.release_id,
                "artifact": sample.artifact,
                "snapshot": sample.snapshot,
            },
        }))?
    );
    Ok(())
}
