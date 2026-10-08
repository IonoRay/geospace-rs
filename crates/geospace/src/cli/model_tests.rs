use super::*;
use serde_json::json;

fn request(model: &str) -> Value {
    json!({"id":"sample", "model":model, "mode":"auto", "data_policy":"offline",
        "at":"2020-07-01T12:00:00Z", "latitude_deg":30, "longitude_deg":120, "altitude_km":300})
}

async fn records(bytes: &[u8], batch: bool) -> (ExecuteStatus, Vec<Value>) {
    let home = tempfile::tempdir().unwrap();
    let mut output = Vec::new();
    let status = process(Some(home.path()), bytes, &mut output, batch)
        .await
        .unwrap();
    let rows = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (status, rows)
}

#[tokio::test]
async fn jsonl_continues_after_bad_record() {
    let good = request("igrf14").to_string();
    let input = format!("{good}\r\n\n{{bad}}\n").into_bytes();
    let input = [input, vec![0xff, b'\n'], good.into_bytes()].concat();
    let (status, rows) = records(&input, true).await;
    assert_eq!(status, ExecuteStatus::RecordFailures);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["status"], "succeeded");
    for (offset, row) in rows[1..4].iter().enumerate() {
        assert_eq!(row["line"], offset + 2);
        assert_eq!(row["error"]["code"], "invalid_json");
    }
    assert_eq!(rows[4]["status"], "succeeded");
    assert_eq!(rows[4]["line"], 5);
}

#[tokio::test]
async fn single_pretty_json_and_empty_batch() {
    let (_, rows) = records(
        serde_json::to_string_pretty(&request("igrf14"))
            .unwrap()
            .as_bytes(),
        false,
    )
    .await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["status"], "succeeded");
    let (status, rows) = records(b"", true).await;
    assert_eq!(status, ExecuteStatus::Succeeded);
    assert_eq!(rows.len(), 0);
    let (_, rows) = records(b"", false).await;
    assert_eq!(rows[0]["error"]["code"], "invalid_json");
}

#[tokio::test]
async fn rejects_structure_duplicate_foreign_driver_and_non_utc() {
    let mut cases = Vec::new();
    let mut value = request("igrf14");
    value["unknown"] = json!(1);
    cases.push(value);
    let mut value = request("hwm14");
    value["drivers"] = json!({"activity":"quiet", "f107_daily":null});
    cases.push(value);
    let mut value = request("hwm14");
    value["drivers"] = json!({"current_ap":2});
    cases.push(value);
    let mut value = request("igrf14");
    value["at"] = json!("2020-07-01T12:00:00 TAI");
    cases.push(value);
    let mut value = request("igrf14");
    value["mode"] = json!("direct");
    value["data_policy"] = Value::Null;
    cases.push(value);
    let mut value = request("iri2020");
    value["mode"] = json!("direct");
    value.as_object_mut().unwrap().remove("data_policy");
    cases.push(value);
    for value in cases {
        let (_, rows) = records(value.to_string().as_bytes(), true).await;
        assert_eq!(rows[0]["id"], "sample");
        assert_eq!(rows[0]["error"]["code"], "invalid_request", "{rows:?}");
    }
    for duplicate in [
        "\"id\":null,\"id\":null,",
        "\"drivers\":null,\"drivers\":null,",
    ] {
        let text = request("igrf14")
            .to_string()
            .replacen('{', &format!("{{{duplicate}"), 1);
        let (_, rows) = records(text.as_bytes(), true).await;
        assert_eq!(rows[0]["status"], "failed");
    }
}

#[tokio::test]
async fn explicit_results_match_prepared_shapes_without_store_access() {
    let home = tempfile::NamedTempFile::new().unwrap(); // Opening it as a data home would fail.
    let mut session = Session::new(Some(home.path()));
    let cases = [
        (
            "iri2020",
            json!({"rz12":5.940_666_666_666_667,"ig12":-5.526_666_666_666_667,"f107_daily":71.2,"f107_81_day":72.1}),
        ),
        ("hwm14", json!({"activity":"quiet"})),
        (
            "nrlmsis21",
            json!({"f107a":69.954_320_987_654_3,"f107_previous_day":68.1,"ap_daily":4}),
        ),
    ];
    for (model, drivers) in cases {
        let mut r = request(model);
        r["drivers"] = drivers;
        let row = evaluate_line(&mut session, 1, &r.to_string()).await;
        assert_eq!(row.status, "succeeded");
        assert!(session.opened.is_none());
        let evaluation = row.evaluation.unwrap();
        if model == "iri2020" {
            assert_eq!(
                evaluation["indices"],
                json!({"rz12":null,"ig12":null,"f107_daily":null,"f107_81_day":null})
            );
        } else if model == "nrlmsis21" {
            assert_eq!(
                evaluation["indices"],
                json!({"f107a":null,"f107_previous_day":null,"ap_daily":null,"ap_three_hourly":[]})
            );
        }
    }
}

#[tokio::test]
async fn batch_reuses_store_and_caches_failed_open() {
    let home = tempfile::tempdir().unwrap();
    let mut session = Session::new(Some(home.path()));
    let text = request("hwm14").to_string();
    let first = evaluate_line(&mut session, 1, &text).await;
    assert_eq!(first.status, "succeeded");
    let store = std::ptr::from_ref(session.get().await.unwrap().indices());
    let second = evaluate_line(&mut session, 2, &text).await;
    assert_eq!(first.evaluation, second.evaluation);
    assert_eq!(
        store,
        std::ptr::from_ref(session.get().await.unwrap().indices())
    );

    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("home");
    std::fs::write(&path, b"not a directory").unwrap();
    let mut session = Session::new(Some(&path));
    let first = evaluate_line(&mut session, 1, &text).await;
    assert_eq!(first.error.as_ref().unwrap().code, "data_access");
    std::fs::remove_file(&path).unwrap(); // Now opening would work, but must not be retried.
    let second = evaluate_line(&mut session, 2, &text).await;
    assert_eq!(first.error.unwrap().message, second.error.unwrap().message);
    assert!(!path.exists());
    assert_eq!(
        evaluate_line(&mut session, 3, &request("igrf14").to_string())
            .await
            .status,
        "succeeded"
    );
}

#[tokio::test]
async fn stream_io_failure_is_command_error() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic write failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic read failure"))
        }
    }
    impl BufRead for Broken {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("synthetic read failure"))
        }
        fn consume(&mut self, _: usize) {}
    }
    assert!(process(None, Broken, Vec::new(), true).await.is_err());
    let input = request("igrf14").to_string();
    assert!(process(None, input.as_bytes(), Broken, true).await.is_err());
}
