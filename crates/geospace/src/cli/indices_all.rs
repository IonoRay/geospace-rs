use std::path::Path;

use ionoray_indices::{
    IndexStore,
    activity::{ActivityDataset, ActivityRange, ActivityStore},
};
use ionoray_omni::{OmniRange, OmniStore};
use serde_json::{Value, json};

use super::{indices::CliSyncPolicy, output};
use crate::GeospaceError;

pub(super) async fn execute(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
) -> Result<(), GeospaceError> {
    let (report, failed) = synchronize(home, year, policy).await?;
    output::json(&report)?;
    if failed > 0 {
        return Err(GeospaceError::AnnualBatchFailures(failed));
    }
    Ok(())
}

async fn synchronize(
    home: Option<&Path>,
    year: u16,
    policy: CliSyncPolicy,
) -> Result<(Value, usize), GeospaceError> {
    if !(1900..=9998).contains(&year) {
        return Err(GeospaceError::InvalidYear(i32::from(year)));
    }
    let start = format!("{year:04}-01-01T00:00:00Z");
    let end = format!("{:04}-01-01T00:00:00Z", year + 1);
    let range = ActivityRange::parse(&start, &end)?;
    let omni_range = OmniRange::parse(&start, &end)?;
    let mut reports = Vec::new();
    let mut failed = 0;
    let core = async {
        let store = IndexStore::open(home).await?;
        Ok(serde_json::to_value(
            store.sync_year(year, policy.into()).await?,
        )?)
    }
    .await;
    record(&mut reports, &mut failed, "core-indices", core);
    for dataset in [
        ActivityDataset::Hp30,
        ActivityDataset::Hp60,
        ActivityDataset::Pcn,
    ] {
        let result = async {
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
        .await;
        record(&mut reports, &mut failed, dataset.name(), result);
    }
    let omni = async {
        let store = OmniStore::open(home).await?;
        match policy {
            CliSyncPolicy::Offline => Ok(serde_json::to_value(
                store.read(omni_range, None, |_| Ok(())).await?,
            )?),
            _ => Ok(serde_json::to_value(
                store
                    .download(
                        omni_range,
                        None,
                        matches!(policy, CliSyncPolicy::ForceDownload),
                    )
                    .await?,
            )?),
        }
    }
    .await;
    record(&mut reports, &mut failed, "omni", omni);
    Ok((
        json!({"year":year,"policy":policy_name(policy),"products":reports,"failed_groups":failed}),
        failed,
    ))
}

fn policy_name(policy: CliSyncPolicy) -> &'static str {
    match policy {
        CliSyncPolicy::AlwaysCheck => "always-check",
        CliSyncPolicy::ForceDownload => "force-download",
        CliSyncPolicy::Offline => "offline",
    }
}

fn record(
    reports: &mut Vec<Value>,
    failed: &mut usize,
    product: &str,
    result: Result<Value, GeospaceError>,
) {
    reports.push(match result {
        Ok(report) => json!({"product":product,"status":"succeeded","report":report}),
        Err(error) => {
            *failed += 1;
            json!({"product":product,"status":"failed","error":error.to_string()})
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn offline_all_attempts_every_group_after_failures_without_network() {
        let home = tempfile::tempdir().unwrap();
        let (report, failed) = synchronize(Some(home.path()), 2024, CliSyncPolicy::Offline)
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
            synchronize(None, 9999, CliSyncPolicy::Offline).await,
            Err(GeospaceError::InvalidYear(9999))
        ));
    }

    #[test]
    fn successful_reports_and_failed_groups_remain_distinct() {
        let mut reports = Vec::new();
        let mut failed = 0;
        record(&mut reports, &mut failed, "hp30", Ok(json!({"files":[]})));
        record(
            &mut reports,
            &mut failed,
            "omni",
            Err(GeospaceError::InvalidYear(0)),
        );
        assert_eq!(failed, 1);
        assert_eq!(reports[0]["status"], "succeeded");
        assert_eq!(reports[1]["status"], "failed");
    }
}
