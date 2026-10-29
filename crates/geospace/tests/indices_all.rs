//! Unified yearly launch and CLI failure contract, entirely offline.
#![cfg(all(feature = "cli", feature = "indices"))]

use std::process::Command;

#[test]
fn annual_launch_routes_all_groups_and_reports_failures_without_stopping_early() {
    let launch: serde_json::Value =
        serde_json::from_str(include_str!("../../../.vscode/launch.json")).unwrap();
    let configs = launch["configurations"].as_array().unwrap();
    for removed in [
        "Geospace indices: download Hpo/PCN date range (online)",
        "Geospace OMNI: download date range (online)",
    ] {
        assert!(configs.iter().all(|c| c["name"] != removed));
    }
    let config = configs
        .iter()
        .find(|c| c["name"] == "Geospace Data: sync all indices for year (online)")
        .unwrap();
    let home = tempfile::tempdir().unwrap();
    let args: Vec<_> = config["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            a.as_str()
                .unwrap()
                .replace("${input:indicesHome}", home.path().to_str().unwrap())
                .replace("${input:indicesYear}", "2024")
        })
        .collect();
    let output = Command::new(env!("CARGO_BIN_EXE_geospace"))
        .args(args)
        .args(["--policy", "offline"])
        .env("IONORAY_LOG_CONSOLE", "off")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["year"], 2024);
    assert_eq!(report["policy"], "offline");
    assert_eq!(report["failed_groups"], 5);
    let products = report["products"].as_array().unwrap();
    assert_eq!(
        products
            .iter()
            .map(|p| p["product"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["core-indices", "hp30", "hp60", "pcn", "omni"]
    );
    assert!(
        products
            .iter()
            .all(|p| p["status"] == "failed" && p["error"].is_string())
    );
}
