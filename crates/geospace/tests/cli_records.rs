//! Real process exit codes and compile-time feature boundaries; no HTTP.
#![cfg(feature = "cli")]

use serde_json::{Value, json};
#[cfg(feature = "cli-standard")]
use std::path::Path;
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

#[cfg(feature = "cli-standard")]
#[path = "../examples/support/automatic_fixture.rs"]
mod fixture;

fn run(args: &[&str], input: &[u8]) -> Output {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_geospace"))
        .args(args)
        .env("IONORAY_HOME", home.path())
        .env("IONORAY_OFFLINE", "1")
        .env("IONORAY_LOG_CONSOLE", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(feature = "cli-standard")]
fn batch_fixture(home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_geospace"))
        .args([
            "--home",
            home.to_str().unwrap(),
            "model",
            "batch",
            "--input",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/model_batch.jsonl"
            ),
        ])
        .env("IONORAY_OFFLINE", "1")
        .env("IONORAY_LOG_CONSOLE", "off")
        .output()
        .unwrap()
}

#[cfg(feature = "cli-standard")]
fn assert_evaluation(actual: &Value, expected: &Value) {
    assert_eq!(actual["input"], expected["input"]);
    let mut actual_evidence = actual.as_object().unwrap().clone();
    let mut expected_evidence = expected.as_object().unwrap().clone();
    actual_evidence.remove("input");
    actual_evidence.remove("result");
    expected_evidence.remove("input");
    expected_evidence.remove("result");
    assert_eq!(actual_evidence, expected_evidence);
    assert_result_close(&actual["result"], &expected["result"]);
}

#[cfg(feature = "cli-standard")]
fn assert_result_close(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() <= 1e-12 * b.abs().max(1e-12), "{a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_result_close(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, value) in a {
                assert_result_close(value, &b[key]);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

fn request(model: &str, drivers: &Value) -> Value {
    json!({"id":"fixture", "model":model, "mode":"direct", "drivers":drivers,
        "at":"2020-07-01T12:00:00Z", "latitude_deg":30, "longitude_deg":120, "altitude_km":300})
}

#[test]
fn command_exit_codes_and_removed_point() {
    assert_eq!(run(&["capabilities"], b"").status.code(), Some(0));
    assert_eq!(
        run(&["model", "run", "--input", "/dev/null/missing"], b"")
            .status
            .code(),
        Some(1)
    );
    assert_eq!(run(&["point", "run"], b"").status.code(), Some(2));
    let output = run(&["model", "batch", "--input", "-"], b"{bad}\n");
    assert_eq!(output.status.code(), Some(3));
    let row: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["error"]["code"], "invalid_json");
    assert_eq!(
        run(&["model", "batch", "--input", "-"], b"").status.code(),
        Some(0)
    );
}

#[test]
fn single_file_keeps_json_stdout_separate_from_traces() {
    let home = tempfile::tempdir().unwrap();
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), request("igrf14", &json!({})).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_geospace"))
        .args([
            "--home",
            home.path().to_str().unwrap(),
            "model",
            "run",
            "--input",
            file.path().to_str().unwrap(),
        ])
        .env("IONORAY_OFFLINE", "1")
        .env("IONORAY_LOG_CONSOLE", "on")
        .env("IONORAY_LOG_FORMAT", "json")
        .env("RUST_LOG", "ionoray_igrf=debug")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(if cfg!(feature = "igrf") { 0 } else { 3 })
    );
    let row: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["id"], "fixture");
    assert_eq!(row["line"], 1);
    assert!(!output.stderr.is_empty());
    for line in String::from_utf8(output.stderr).unwrap().lines() {
        let event: Value = serde_json::from_str(line).unwrap();
        assert!(event.is_object());
    }
}

#[test]
fn compiled_models_run_direct_without_indices_and_missing_models_are_explicit() {
    let cases = [
        ("igrf14", cfg!(feature = "igrf"), json!({})),
        (
            "iri2020",
            cfg!(feature = "iri"),
            json!({"rz12":5.9,"ig12":-5.5,"f107_daily":71.2,"f107_81_day":72.1}),
        ),
        ("hwm14", cfg!(feature = "hwm"), json!({"activity":"quiet"})),
        (
            "nrlmsis21",
            cfg!(feature = "msis"),
            json!({"f107a":70,"f107_previous_day":68.1,"ap_daily":4}),
        ),
    ];
    for (model, compiled, drivers) in cases {
        let output = run(
            &["model", "run", "--input", "-"],
            request(model, &drivers).to_string().as_bytes(),
        );
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(if compiled { 0 } else { 3 }));
        if compiled {
            assert_eq!(row["status"], "succeeded", "{row}");
        } else {
            assert_eq!(row["error"]["code"], "model_unavailable", "{row}");
        }
    }
}

#[test]
fn incomplete_auto_driver_requires_indices_feature() {
    for (model, compiled) in [
        ("iri2020", cfg!(feature = "iri")),
        ("hwm14", cfg!(feature = "hwm")),
        ("nrlmsis21", cfg!(feature = "msis")),
    ] {
        let mut value = request(model, &json!({}));
        value["mode"] = json!("auto");
        value["data_policy"] = json!("offline");
        let output = run(
            &["model", "run", "--input", "-"],
            value.to_string().as_bytes(),
        );
        let row: Value = serde_json::from_slice(&output.stdout).unwrap();
        if !compiled {
            assert_eq!(row["error"]["code"], "model_unavailable");
        } else if !cfg!(feature = "indices") {
            assert_eq!(row["error"]["code"], "data_unavailable");
        } else {
            assert_eq!(row["status"], "succeeded", "{row}");
        }
    }
}

#[test]
#[cfg(feature = "igrf")]
fn binary_preserves_success_failure_success_and_physical_lines() {
    let good = request("igrf14", &json!({}));
    let mut bad = good.clone();
    bad["latitude_deg"] = json!(91);
    let output = run(
        &["model", "batch", "--input", "-"],
        format!("{good}\n{bad}\n{good}").as_bytes(),
    );
    assert_eq!(output.status.code(), Some(3));
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["evaluation"], rows[2]["evaluation"]);
    assert_eq!(rows[1]["error"]["code"], "invalid_request");
    assert_eq!(rows[2]["line"], 3);
}

#[tokio::test]
#[cfg(feature = "cli-standard")]
#[expect(
    clippy::too_many_lines,
    reason = "one file batch compares all model records"
)]
async fn file_batch_matches_rust_models_and_offline_preparation() {
    use ionoray_geospace::{
        DataPolicy, Geospace, HwmRequest, IriRequest, MsisRequest,
        hwm::{Hwm, HwmGeomagneticActivity, HwmInput, HwmVersion},
        igrf::{Igrf, IgrfInput, IgrfVersion},
        iri::{Iri, IriDrivers, IriInput, IriVersion},
        msis::{Msis, MsisApHistory, MsisDrivers, MsisGeomagneticActivity, MsisInput, MsisVersion},
    };

    let home = tempfile::tempdir().unwrap();
    let output = batch_fixture(home.path());
    assert_eq!(output.status.code(), Some(3));
    let rows: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let requests: Vec<Value> = include_str!("../../../examples/model_batch.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), requests.len());
    for (index, (row, request)) in rows.iter().zip(&requests).enumerate() {
        assert_eq!(row["line"], index + 1);
        assert_eq!(row["id"], request["id"]);
        assert_eq!(row["model"], request["model"]);
        assert_eq!(row["schema_version"], 1);
    }

    let query = fixture::query();
    let igrf_input = IgrfInput { query };
    let igrf = Igrf::new(IgrfVersion::Igrf14).unwrap();
    let igrf_expected = json!({"input":igrf_input,"result":igrf.evaluate(&igrf_input).unwrap()});
    assert_evaluation(&rows[0]["evaluation"], &igrf_expected);

    let iri_input = IriInput {
        query,
        drivers: IriDrivers {
            sunspot_number_12_month: 5.940_666_666_666_667,
            ionospheric_index_12_month: -5.526_666_666_666_667,
            f107_daily: 71.2,
            f107_81_day: 72.1,
        },
    };
    assert_evaluation(
        &rows[1]["evaluation"],
        &json!({"input":iri_input,"indices":{"rz12":null,"ig12":null,"f107_daily":null,"f107_81_day":null},
            "result":Iri::new(IriVersion::Iri2020).evaluate(&iri_input).unwrap()}),
    );

    let hwm_input = HwmInput {
        query,
        geomagnetic_activity: HwmGeomagneticActivity::Disturbed { current_ap: 2.0 },
    };
    assert_evaluation(
        &rows[2]["evaluation"],
        &json!({"input":hwm_input,"ap_index":null,
            "result":Hwm::new(HwmVersion::Hwm14).evaluate(&hwm_input).unwrap()}),
    );

    let msis_input = MsisInput {
        query,
        drivers: MsisDrivers {
            f107a: 69.954_320_987_654_3,
            f107_previous_day: 68.1,
            geomagnetic_activity: MsisGeomagneticActivity::StormTime(MsisApHistory {
                daily: 4.0,
                current: 2.0,
                three_hours_ago: 4.0,
                six_hours_ago: 3.0,
                nine_hours_ago: 4.0,
                average_12_to_33_hours: 3.375,
                average_36_to_57_hours: 2.5,
            }),
        },
    };
    assert_evaluation(
        &rows[3]["evaluation"],
        &json!({"input":msis_input,
            "indices":{"f107a":null,"f107_previous_day":null,"ap_daily":null,"ap_three_hourly":[]},
            "result":Msis::new(MsisVersion::Nrlmsis21).evaluate(&msis_input).unwrap()}),
    );

    let geospace = Geospace::open(Some(home.path())).await.unwrap();
    assert_evaluation(
        &rows[4]["evaluation"],
        &json!(
            geospace
                .evaluate_iri(IriRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        ),
    );
    assert_evaluation(
        &rows[5]["evaluation"],
        &json!(
            geospace
                .evaluate_hwm(HwmRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        ),
    );
    assert_evaluation(
        &rows[6]["evaluation"],
        &json!(
            geospace
                .evaluate_msis(MsisRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        ),
    );
    assert!(rows[4]["evaluation"]["indices"]["ig12"].is_object());
    assert!(rows[5]["evaluation"]["ap_index"].is_object());
    assert_eq!(
        rows[6]["evaluation"]["indices"]["ap_three_hourly"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    let ap_values: Vec<f64> = rows[6]["evaluation"]["indices"]["ap_three_hourly"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sample| sample["value"].as_f64().unwrap())
        .collect();
    assert_eq!(ap_values, fixture::AP_HISTORY);

    for row in &rows[7..11] {
        assert_eq!(row["status"], "failed");
        assert_eq!(row["error"]["code"], "invalid_request");
        assert!(row["evaluation"].is_null());
    }
    assert_eq!(rows[11]["status"], "failed");
    assert_eq!(rows[11]["error"]["code"], "data_unavailable");
    assert_eq!(rows[12]["status"], "succeeded");
    assert_evaluation(&rows[12]["evaluation"], &igrf_expected);
}
