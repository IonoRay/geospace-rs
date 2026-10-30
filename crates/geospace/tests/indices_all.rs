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
    let log = home.path().join("annual.ndjson");
    assert_eq!(
        config["env"]["IONORAY_LOG_FILE"],
        "${workspaceFolder}/logs/indices-sync.ndjson"
    );
    let args: Vec<_> = config["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            a.as_str()
                .unwrap()
                .replace("${input:indicesHome}", home.path().to_str().unwrap())
                .replace("${input:indicesYear}", "2024")
                .replace("${input:indicesSyncDataset}", "all")
        })
        .collect();
    let output = Command::new(env!("CARGO_BIN_EXE_geospace"))
        .args(args)
        .args(["--policy", "offline"])
        .env("IONORAY_LOG_CONSOLE", "off")
        .env("IONORAY_LOG_FILE", &log)
        .env("RUST_LOG", "info")
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
    assert_logs(&log, home.path(), products);
}

#[test]
fn annual_choices_sync_only_the_selected_dataset_and_year() {
    let launch: serde_json::Value =
        serde_json::from_str(include_str!("../../../.vscode/launch.json")).unwrap();
    let input = launch["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "indicesSyncDataset")
        .unwrap();
    assert_eq!(input["type"], "pickString");
    assert_eq!(input["default"], "all");
    assert_eq!(input["options"][0], "all");
    assert_eq!(input["options"].as_array().unwrap().len(), 10);
    for dataset in input["options"].as_array().unwrap().iter().skip(1) {
        let dataset = dataset.as_str().unwrap();
        let home = tempfile::tempdir().unwrap();
        let log = home.path().join("run.ndjson");
        let output = Command::new(env!("CARGO_BIN_EXE_geospace"))
            .args([
                "--home",
                home.path().to_str().unwrap(),
                "indices",
                "sync",
                "--dataset",
                dataset,
                "--year",
                "2025",
                "--policy",
                "offline",
            ])
            .env("IONORAY_LOG_CONSOLE", "off")
            .env("IONORAY_LOG_FILE", &log)
            .env("RUST_LOG", "info")
            .output()
            .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["dataset"], dataset);
        assert_eq!(report["year"], 2025);
        assert_eq!(report["products"].as_array().unwrap().len(), 1);
        assert_eq!(report["products"][0]["product"], dataset);
        let failed = report["products"][0]["status"] == "failed";
        assert_eq!(output.status.code(), Some(i32::from(failed)));
        assert_eq!(report["failed_groups"], usize::from(failed));
        let events: Vec<serde_json::Value> = std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let groups: Vec<_> = events
            .iter()
            .filter(|e| e["fields"]["message"] == "annual product group started")
            .collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["fields"]["group"], dataset);
        assert_eq!(groups[0]["fields"]["group_index"], 1);
        assert_eq!(groups[0]["fields"]["total_groups"], 1);
        assert_only_selected_scope(home.path(), dataset);
    }
}

fn assert_only_selected_scope(home: &std::path::Path, dataset: &str) {
    let objects = home.join("objects");
    if objects.exists() {
        for entry in std::fs::read_dir(objects).unwrap() {
            assert_eq!(
                entry.unwrap().file_name(),
                dataset,
                "unselected dataset was opened"
            );
        }
    }
    assert_eq!(home.join("omni").exists(), dataset == "omni");
}

fn assert_logs(log: &std::path::Path, home: &std::path::Path, products: &[serde_json::Value]) {
    let events: Vec<serde_json::Value> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let fields: Vec<_> = events.iter().map(|event| &event["fields"]).collect();
    let start = fields
        .iter()
        .find(|f| f["message"] == "annual synchronization started")
        .unwrap();
    assert_eq!(start["home"], home.to_str().unwrap());
    assert_eq!(start["total_groups"], 5);
    assert_eq!(start["policy"], "offline");
    let groups: Vec<_> = fields
        .iter()
        .filter(|f| f["message"] == "annual product group started")
        .collect();
    assert_eq!(groups.len(), 5);
    for (index, group) in groups.iter().enumerate() {
        assert_eq!(group["group_index"], index + 1);
        assert_eq!(group["group"], products[index]["product"]);
    }
    let end = fields
        .iter()
        .find(|f| f["message"] == "annual synchronization finished")
        .unwrap();
    assert_eq!(end["failed_groups"], 5);
    assert!(end["elapsed_secs"].is_u64());
    let counts: Vec<_> = fields
        .iter()
        .filter(|f| f["message"] == "core dataset processing counts")
        .collect();
    assert_eq!(counts.len(), 5);
    for count in counts {
        for name in [
            "records_imported",
            "bodies_downloaded",
            "bytes_downloaded",
            "releases_created",
            "releases_reused",
        ] {
            assert!(count[name].is_u64(), "missing {name}: {count}");
        }
        assert_eq!(count["bytes_downloaded"], 0);
        assert_eq!(count["bodies_downloaded"], 0);
    }
    for dataset in ["kp-ap-f107", "dst", "ae", "iri-ig-rz", "iri-apf107"] {
        // Build-time packaged sources can satisfy the non-AE datasets offline.
        let status = if dataset == "ae" {
            "failed"
        } else {
            "succeeded"
        };
        assert!(
            fields
                .iter()
                .any(|f| f["message"] == "annual core dataset finished"
                    && f["dataset"] == dataset
                    && f["status"] == status),
            "missing result for {dataset}"
        );
    }
    let summary = fields
        .iter()
        .find(|f| f["message"] == "annual synchronization summary")
        .unwrap();
    assert!(
        summary["summary"]
            .as_str()
            .unwrap()
            .contains("failed=[core-indices:")
    );
    let failure = fields
        .iter()
        .find(|f| f["message"] == "command failed")
        .unwrap();
    let error = failure["error"].as_str().unwrap();
    for dataset in [
        "kp-ap-f107",
        "dst",
        "ae",
        "iri-ig-rz",
        "iri-apf107",
        "hp30",
        "hp60",
        "pcn",
        "omni",
    ] {
        assert!(error.contains(dataset), "final error omitted {dataset}");
    }
}
