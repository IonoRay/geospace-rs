//! Small sequential maintenance schedule shared by the example and test.
//!
//! It deliberately delegates every operation to `IndexStore`; it is not a
//! queue, a retry policy, or persistent job state.

use ionoray_indices::{IndexStore, RangeRequest, RangeSyncReport, SyncPolicy, SyncReport};
use serde::Serialize;

/// One explicit maintenance operation, intended for a caller-owned schedule.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum MaintenanceJob {
    /// Synchronize the stated fields over one UTC half-open interval.
    #[allow(
        dead_code,
        reason = "available to caller-owned schedules and exercised by integration tests"
    )]
    Range {
        /// Caller-assigned stable label for reporting.
        id: &'static str,
        /// Exact fields, interval, and network policy.
        #[serde(serialize_with = "serialize_request")]
        request: RangeRequest,
    },
    /// Synchronize every supported dataset for one calendar year.
    Year {
        /// Caller-assigned stable label for reporting.
        id: &'static str,
        /// Calendar year to maintain.
        year: u16,
        /// Upstream policy; callers choose network access explicitly.
        policy: SyncPolicy,
    },
}

/// Per-job result retained even when a preceding job reported an error.
#[derive(Debug, Serialize)]
pub struct JobOutcome {
    /// Original request and policy, retained even when execution fails.
    #[serde(flatten)]
    pub job: MaintenanceJob,
    /// Successful production report, including a possibly partial range report.
    pub report: Option<JobReport>,
    /// String form of an infrastructure error; no later job is suppressed.
    pub error: Option<String>,
}

/// The two production reports this small scheduler can emit.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum JobReport {
    /// Result of one bounded field request.
    Range(RangeSyncReport),
    /// Result of one all-dataset annual synchronization.
    Year(SyncReport),
}

/// Runs jobs in input order without retrying, parallelizing, or stopping early.
pub async fn run_jobs(store: &IndexStore, jobs: Vec<MaintenanceJob>) -> Vec<JobOutcome> {
    let mut outcomes = Vec::with_capacity(jobs.len());
    for job in jobs {
        let result = match &job {
            MaintenanceJob::Range { request, .. } => store
                .sync_range(request.clone())
                .await
                .map(JobReport::Range),
            MaintenanceJob::Year { year, policy, .. } => {
                store.sync_year(*year, *policy).await.map(JobReport::Year)
            }
        };
        match result {
            Ok(report) => outcomes.push(JobOutcome {
                job,
                report: Some(report),
                error: None,
            }),
            Err(error) => outcomes.push(JobOutcome {
                job,
                report: None,
                error: Some(error.to_string()),
            }),
        }
    }
    outcomes
}

// Display the caller input without changing the production request/data format.
fn serialize_request<S: serde::Serializer>(
    request: &RangeRequest,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde_json::json!({
        "dataset": request.dataset, "fields": request.fields,
        "start": request.start.to_string(), "end": request.end.to_string(),
        "mode": request.mode, "force": request.force,
    })
    .serialize(serializer)
}
