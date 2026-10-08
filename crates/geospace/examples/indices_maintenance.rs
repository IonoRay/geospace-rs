//! Synchronize all index datasets for an explicit UTC-year range.
//!
//! This is an online-maintenance entry point. It requires an explicit store
//! home and either `--year YEAR` or `--start-year YEAR --end-year YEAR`; it
//! never chooses a default home or year range. For offline typed reads and
//! coverage gaps, use `indices_offline` instead.

#[path = "support/maintenance_args.rs"]
mod maintenance_args;
#[path = "support/maintenance_schedule.rs"]
mod schedule;

use clap::Parser;
use ionoray_core::Epoch;
use ionoray_indices::IndexStore;
use ionoray_store::StoreHome;
use maintenance_args::{MaintenanceArgs, selected_years};
use schedule::{MaintenanceJob, run_jobs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Clap prints help/errors and exits before any store is opened.
    let args = MaintenanceArgs::parse();
    let current_year = u16::try_from(Epoch::now()?.to_gregorian_utc().0)?;
    let years = selected_years(&args, current_year)?;
    // Breakpoint 1: inspect the explicit directory, selected years, and policy.
    let home = StoreHome::discover(Some(&args.home))?;
    let policy = args.policy.into();

    eprintln!("resolved data home (--home): {}", home.as_path().display());
    eprintln!("UTC years: {}..={}", years.start(), years.end());
    eprintln!("Sync policy: {policy:?}");
    eprintln!("Runtime network allowed: {}", args.policy.permits_network());

    // Breakpoint 2: the resolved home is printed before any store writes.
    let store = IndexStore::open(Some(home.as_path())).await?;

    let mut failed_years = Vec::new();
    for year in years {
        eprintln!("Synchronizing all indices for {year}");
        let job = MaintenanceJob::Year {
            id: "annual_refresh",
            year,
            policy,
        };
        // Breakpoint 3: inspect year and the report/error before the next year.
        for outcome in run_jobs(&store, vec![job]).await {
            if outcome.error.is_some() {
                failed_years.push(year);
            }
            println!("{}", serde_json::to_string(&outcome)?);
        }
    }
    if !failed_years.is_empty() {
        return Err(format!("annual synchronization failed for years: {failed_years:?}").into());
    }
    Ok(())
}
