use std::path::Path;

use ionoray_indices::{
    IndexStore,
    activity::{ActivityDataset, ActivityRange, ActivityStore},
};
use ionoray_omni::{OmniRange, OmniStore};
use serde_json::{Value, json};

use super::{indices::CliSyncPolicy, indices_selection::AnnualDataset, output};
use crate::GeospaceError;

pub(super) async fn execute(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
    selection: AnnualDataset,
) -> Result<(), GeospaceError> {
    let started = std::time::Instant::now();
    let (report, failed) = synchronize(home, year, policy, selection).await?;
    output::json(&report)?;
    let summary = log_summary(&report);
    tracing::info!(
        year,
        elapsed_secs = started.elapsed().as_secs(),
        failed_groups = failed,
        "annual synchronization finished"
    );
    if failed > 0 {
        return Err(GeospaceError::AnnualBatchFailures(summary));
    }
    Ok(())
}

async fn synchronize(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
    selection: AnnualDataset,
) -> Result<(Value, usize), GeospaceError> {
    if !(1900..=9998).contains(&year) {
        return Err(GeospaceError::InvalidYear(i32::from(year)));
    }
    let resolved_home = ionoray_store::StoreHome::discover(home)?;
    let total_groups = selection.total_groups();
    let groups = if selection == AnnualDataset::All {
        "core-indices,hp30,hp60,pcn,omni"
    } else {
        selection.name()
    };
    tracing::info!(year, policy = policy_name(policy), dataset = selection.name(), home = %resolved_home.as_path().display(), total_groups, groups, "annual synchronization started");
    let mut reports = Vec::new();
    let mut failed = 0;
    if selection == AnnualDataset::All || selection.core().is_some() {
        let name = if selection == AnnualDataset::All {
            "core-indices"
        } else {
            selection.name()
        };
        log_group_start(name, 1, total_groups);
        let started = std::time::Instant::now();
        let result = sync_core(home, year, policy, selection).await;
        record(
            &mut reports,
            &mut failed,
            name,
            result,
            started.elapsed().as_secs(),
        );
    }
    for dataset in [
        ActivityDataset::Hp30,
        ActivityDataset::Hp60,
        ActivityDataset::Pcn,
    ]
    .into_iter()
    .filter(|dataset| selection.includes(dataset.name()))
    {
        log_group_start(dataset.name(), reports.len() + 1, total_groups);
        let started = std::time::Instant::now();
        let result = sync_activity(home, year, policy, dataset).await;
        record(
            &mut reports,
            &mut failed,
            dataset.name(),
            result,
            started.elapsed().as_secs(),
        );
    }
    if selection.includes("omni") {
        log_group_start("omni", reports.len() + 1, total_groups);
        let started = std::time::Instant::now();
        let result = sync_omni(home, year, policy).await;
        record(
            &mut reports,
            &mut failed,
            "omni",
            result,
            started.elapsed().as_secs(),
        );
    }
    Ok((
        json!({"year":year,"dataset":selection.name(),"policy":policy_name(policy),"products":reports,"failed_groups":failed}),
        failed,
    ))
}

async fn sync_core(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
    selection: AnnualDataset,
) -> Result<Value, GeospaceError> {
    let store = IndexStore::open(home).await?;
    let report = if let Some(dataset) = selection.core() {
        store
            .sync_dataset_year(dataset, year, policy.into())
            .await?
    } else {
        store.sync_year(year, policy.into()).await?
    };
    Ok(serde_json::to_value(report)?)
}

async fn sync_activity(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
    dataset: ActivityDataset,
) -> Result<Value, GeospaceError> {
    let range = ActivityRange::parse(
        &format!("{year:04}-01-01T00:00:00Z"),
        &format!("{:04}-01-01T00:00:00Z", year + 1),
    )?;
    let store = ActivityStore::open(home)?;
    let report = match policy {
        CliSyncPolicy::Offline => store.read(dataset, range, |_| Ok(())).await?,
        _ => {
            store
                .download(
                    dataset,
                    range,
                    matches!(policy, CliSyncPolicy::ForceDownload),
                )
                .await?
        }
    };
    Ok(serde_json::to_value(report)?)
}

async fn sync_omni(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
) -> Result<Value, GeospaceError> {
    let range = OmniRange::parse(
        &format!("{year:04}-01-01T00:00:00Z"),
        &format!("{:04}-01-01T00:00:00Z", year + 1),
    )?;
    let store = OmniStore::open(home).await?;
    match policy {
        CliSyncPolicy::Offline => Ok(serde_json::to_value(
            store.read(range, None, |_| Ok(())).await?,
        )?),
        _ => Ok(serde_json::to_value(
            store
                .download(range, None, matches!(policy, CliSyncPolicy::ForceDownload))
                .await?,
        )?),
    }
}

fn policy_name(policy: CliSyncPolicy) -> &'static str {
    match policy {
        CliSyncPolicy::AlwaysCheck => "always-check",
        CliSyncPolicy::ForceDownload => "force-download",
        CliSyncPolicy::Offline => "offline",
    }
}

fn log_group_start(group: &str, group_index: usize, total_groups: usize) {
    tracing::info!(
        group,
        group_index,
        total_groups,
        "annual product group started"
    );
}

fn record(
    reports: &mut Vec<Value>,
    failed: &mut usize,
    product: &str,
    result: Result<Value, GeospaceError>,
    elapsed_secs: u64,
) {
    reports.push(match result {
        Ok(report) => {
            tracing::info!(dataset = product, status = "succeeded", elapsed_secs, "annual product group finished");
            json!({"product":product,"status":"succeeded","report":report})
        }
        Err(error) => {
            *failed += 1;
            tracing::warn!(dataset = product, status = "failed", elapsed_secs, error = %error, "annual product group finished");
            json!({"product":product,"status":"failed","error":error.to_string()})
        }
    });
}

fn log_summary(report: &Value) -> String {
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for product in report["products"].as_array().into_iter().flatten() {
        let name = product["product"].as_str().unwrap_or("unknown");
        if product["status"] == "succeeded" {
            succeeded.push(name);
        } else {
            failed.push(format!(
                "{name}: {}",
                product["error"].as_str().unwrap_or("unknown error")
            ));
        }
    }
    let summary = format!(
        "succeeded=[{}]; failed=[{}]",
        succeeded.join(", "),
        failed.join("; ")
    );
    tracing::info!(year = %report["year"], %summary, "annual synchronization summary");
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn offline_all_attempts_every_group_after_failures_without_network() {
        let home = tempfile::tempdir().unwrap();
        let (report, failed) = synchronize(
            Some(home.path()),
            2024,
            CliSyncPolicy::Offline,
            AnnualDataset::All,
        )
        .await
        .unwrap();
        let products = report["products"].as_array().unwrap();
        assert_eq!(
            products
                .iter()
                .map(|r| r["product"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["core-indices", "hp30", "hp60", "pcn", "omni"]
        );
        assert_eq!(failed, 5);
        assert!(
            products
                .iter()
                .all(|r| r["status"] == "failed" && r["error"].is_string())
        );
        assert_eq!(report["policy"], "offline");
    }

    #[tokio::test]
    async fn invalid_year_is_rejected_before_opening_home() {
        assert!(matches!(
            synchronize(None, 9999, CliSyncPolicy::Offline, AnnualDataset::All).await,
            Err(GeospaceError::InvalidYear(9999))
        ));
    }

    #[test]
    fn successful_reports_and_failed_groups_remain_distinct() {
        let mut reports = Vec::new();
        let mut failed = 0;
        record(
            &mut reports,
            &mut failed,
            "hp30",
            Ok(json!({"files":[]})),
            0,
        );
        record(
            &mut reports,
            &mut failed,
            "omni",
            Err(GeospaceError::InvalidYear(0)),
            0,
        );
        assert_eq!(failed, 1);
        assert_eq!(reports[0]["status"], "succeeded");
        assert_eq!(reports[1]["status"], "failed");
        let summary = log_summary(&json!({"year":2024,"products":reports}));
        assert!(summary.contains("succeeded=[hp30]"));
        assert!(summary.contains("failed=[omni:"));
        assert!(summary.contains(&GeospaceError::InvalidYear(0).to_string()));
    }
}
