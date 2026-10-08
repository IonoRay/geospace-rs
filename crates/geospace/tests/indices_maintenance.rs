//! S8 maintenance schedule contract using the same support code as the example.
#![cfg(feature = "indices")]

#[path = "../examples/support/maintenance_args.rs"]
mod maintenance_args;
#[path = "../examples/support/maintenance_schedule.rs"]
mod schedule;

use clap::Parser;
use ionoray_core::{Duration, Epoch};
use ionoray_indices::{
    CoverageGapReason, IndexDataset, IndexField, IndexStore, RangeRequest, RangeSyncStatus,
    SyncMode,
};
use maintenance_args::{MaintenanceArgs, MaintenancePolicy, selected_years};
use schedule::{JobReport, MaintenanceJob, run_jobs};

#[test]
fn maintenance_arguments_require_explicit_home_and_selection_before_store_opening() {
    for (argument, runtime, permits_network) in [
        (
            MaintenancePolicy::Offline,
            ionoray_indices::SyncPolicy::Offline,
            false,
        ),
        (
            MaintenancePolicy::AlwaysCheck,
            ionoray_indices::SyncPolicy::AlwaysCheck,
            true,
        ),
        (
            MaintenancePolicy::ForceDownload,
            ionoray_indices::SyncPolicy::ForceDownload,
            true,
        ),
    ] {
        assert_eq!(ionoray_indices::SyncPolicy::from(argument), runtime);
        assert_eq!(argument.permits_network(), permits_network);
    }
    assert!(MaintenanceArgs::try_parse_from(["indices_maintenance"]).is_err());
    assert!(
        MaintenanceArgs::try_parse_from([
            "indices_maintenance",
            "--home",
            "/tmp/ionoray-maintenance",
        ])
        .is_err()
    );

    let args = MaintenanceArgs::try_parse_from([
        "indices_maintenance",
        "--home",
        "~/ionoray-maintenance",
        "--year",
        "2020",
        "--policy",
        "offline",
    ])
    .unwrap();
    assert_eq!(args.home.to_string_lossy(), "~/ionoray-maintenance");
    assert_eq!(args.policy, MaintenancePolicy::Offline);
    assert!(!args.policy.permits_network());
    assert_eq!(
        selected_years(&args, 2026).unwrap().collect::<Vec<_>>(),
        [2020]
    );
}

#[test]
fn maintenance_arguments_accept_inclusive_ranges_and_reject_invalid_bounds() {
    let args = MaintenanceArgs::try_parse_from([
        "indices_maintenance",
        "--home",
        "/tmp/ionoray-maintenance",
        "--start-year",
        "1958",
        "--end-year",
        "2026",
        "--policy",
        "force-download",
    ])
    .unwrap();
    assert!(args.policy.permits_network());
    assert_eq!(
        selected_years(&args, 2026).unwrap().collect::<Vec<_>>(),
        (1958..=2026).collect::<Vec<_>>()
    );

    let reversed = MaintenanceArgs::try_parse_from([
        "indices_maintenance",
        "--home",
        "/tmp/ionoray-maintenance",
        "--start-year",
        "2021",
        "--end-year",
        "2020",
    ])
    .unwrap();
    assert!(selected_years(&reversed, 2026).is_err());

    let future = MaintenanceArgs::try_parse_from([
        "indices_maintenance",
        "--home",
        "/tmp/ionoray-maintenance",
        "--year",
        "2027",
    ])
    .unwrap();
    assert!(selected_years(&future, 2026).is_err());

    for invalid in [
        vec!["--end-year", "2020"],
        vec![
            "--year",
            "2020",
            "--start-year",
            "2020",
            "--end-year",
            "2021",
        ],
        vec!["--year", "2020", "--policy", "ensure"],
        vec!["--year", "not-a-year"],
    ] {
        let mut command = vec!["indices_maintenance", "--home", "/tmp/unused"];
        command.extend(invalid);
        assert!(MaintenanceArgs::try_parse_from(command).is_err());
    }
    let early = MaintenanceArgs::try_parse_from([
        "indices_maintenance",
        "--home",
        "/tmp/unused",
        "--year",
        "1957",
    ])
    .unwrap();
    assert!(selected_years(&early, 2026).is_err());
    assert_eq!(early.policy, MaintenancePolicy::AlwaysCheck);
    assert!(early.policy.permits_network());

    assert!(
        MaintenanceArgs::try_parse_from([
            "indices_maintenance",
            "--home",
            "/tmp/ionoray-maintenance",
            "--start-year",
            "2020",
        ])
        .is_err()
    );
}

#[tokio::test]
async fn schedule_preserves_order_and_offline_partial_reports() {
    let home = tempfile::tempdir().unwrap();
    let store = IndexStore::open(Some(home.path())).await.unwrap();
    let start = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
    let jobs = vec![
        range("gfz", IndexDataset::KpApF107, vec![IndexField::Ap3h], start),
        range("dst", IndexDataset::Dst, vec![IndexField::Dst], start),
        range("iri", IndexDataset::IriIgRz, vec![IndexField::Ig12], start),
    ];
    let outcomes = run_jobs(&store, jobs).await;
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| match &outcome.job {
                MaintenanceJob::Range { id, .. } | MaintenanceJob::Year { id, .. } => *id,
            })
            .collect::<Vec<_>>(),
        ["gfz", "dst", "iri"]
    );
    let JobReport::Range(gfz) = outcomes[0].report.as_ref().unwrap() else {
        panic!("range report")
    };
    assert_eq!(gfz.status, RangeSyncStatus::Complete);
    assert_eq!(gfz.download_summary.sources_queried, 0);
    let JobReport::Range(dst) = outcomes[1].report.as_ref().unwrap() else {
        panic!("range report")
    };
    assert_eq!(dst.status, RangeSyncStatus::Partial);
    assert_eq!(dst.download_summary.sources_queried, 0);
    assert!(
        dst.gaps
            .iter()
            .all(|gap| gap.reason == CoverageGapReason::OfflineUnverified)
    );
    let JobReport::Range(iri) = outcomes[2].report.as_ref().unwrap() else {
        panic!("range report")
    };
    assert_eq!(iri.status, RangeSyncStatus::Complete);
    assert_eq!(iri.download_summary.sources_queried, 0);
    assert!(!dst.gaps.is_empty());
    assert!(outcomes.iter().all(|outcome| outcome.error.is_none()));

    // Verbatim GFZ snapshot, line 32365: the 12:00-15:00 UTC ap slot is 2.
    // Source identity is pinned in crates/indices/cache/README.md.
    let ap = store.ap_at(start).await.unwrap();
    assert!((ap.value.value() - 2.0).abs() < f64::EPSILON);
    assert_eq!(ap.interval.start, start);
    assert_eq!(ap.interval.end, start + Duration::from_hours(3.0));
    // The existing reader maps the Rolling edition to Provisional ("r");
    // this checks preserved metadata, not an independent source-quality audit.
    assert_eq!(ap.quality, ionoray_indices::QualityFlag::Provisional);
    assert_eq!(ap.derivation, ionoray_indices::ValueDerivation::Source);
    assert!(!ap.release_id.is_empty());
    assert_eq!(
        ap.artifact.to_hex(),
        "a74cd1096e7b7711690ffba819ddf7149bf32f07a787092510407e5aa1742029"
    );
    assert_ne!(ap.snapshot.as_bytes(), &[0; 32]);
}

fn range(
    id: &'static str,
    dataset: IndexDataset,
    fields: Vec<IndexField>,
    start: Epoch,
) -> MaintenanceJob {
    MaintenanceJob::Range {
        id,
        request: RangeRequest {
            dataset,
            fields,
            start,
            end: start + Duration::from_milliseconds(1.0),
            mode: SyncMode::Offline,
            force: false,
        },
    }
}

#[tokio::test]
async fn schedule_retains_failed_request_and_runs_next_job() {
    let home = tempfile::tempdir().unwrap();
    let store = IndexStore::open(Some(home.path())).await.unwrap();
    let start = "2020-07-01T12:00:00 UTC".parse::<Epoch>().unwrap();
    // Synthetic invalid request: Dst does not own the Ap3h field.
    let jobs = vec![
        range(
            "bad_field",
            IndexDataset::Dst,
            vec![IndexField::Ap3h],
            start,
        ),
        range(
            "after_error",
            IndexDataset::IriIgRz,
            vec![IndexField::Ig12],
            start,
        ),
    ];
    let outcomes = run_jobs(&store, jobs).await;
    let rows = serde_json::to_value(&outcomes).unwrap();
    assert_eq!(rows[0]["id"], "bad_field");
    assert_eq!(rows[0]["operation"], "range");
    assert_eq!(rows[0]["request"]["mode"], "offline");
    assert_eq!(rows[0]["request"]["fields"], serde_json::json!(["ap3h"]));
    assert!(outcomes[0].report.is_none());
    assert!(
        outcomes[0]
            .error
            .as_ref()
            .unwrap()
            .contains("does not belong")
    );
    assert_eq!(rows[1]["id"], "after_error");
    let JobReport::Range(report) = outcomes[1].report.as_ref().unwrap() else {
        panic!("range report")
    };
    assert_eq!(report.status, RangeSyncStatus::Complete);
    assert_eq!(report.download_summary.sources_queried, 0);
}

#[test]
fn annual_template_serializes_explicit_policy_without_execution() {
    let job = MaintenanceJob::Year {
        id: "annual_template",
        year: 2020,
        policy: ionoray_indices::SyncPolicy::AlwaysCheck,
    };
    let value = serde_json::to_value(job).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"operation":"year", "id":"annual_template", "year":2020,"policy":"AlwaysCheck"})
    );
}
