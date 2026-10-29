use super::*;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/activity")
        .join(name)
}

#[test]
fn hp30_matches_official_api_and_preserves_open_ended_values() {
    let range = ActivityRange::parse("2024-05-10", "2024-05-11").unwrap();
    let parsed = parse(
        &fixture("hp30.txt"),
        ActivityDataset::Hp30,
        range,
        "rolling-v3",
    )
    .unwrap();
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../tests/fixtures/activity/hp30-api.json")).unwrap();
    let dates = reference["datetime"].as_array().unwrap();
    for row in &parsed.rows {
        let i = dates
            .iter()
            .position(|s| parse_epoch(s.as_str().unwrap()).unwrap() == row.time)
            .unwrap();
        assert!(
            (row.values["Hp30"].value.unwrap() - reference["Hp30"][i].as_f64().unwrap()).abs()
                < 1e-6
        );
        assert_eq!(row.flags["D"], "0");
        assert_eq!(row.quality, "unspecified_reserved_D_is_not_a_quality_flag");
    }
    assert!(
        parsed
            .rows
            .iter()
            .any(|r| r.values["Hp30"].value.unwrap() > 9.0)
    );
    assert_eq!(parsed.rows[0].values["ap30"].value, Some(6.0));
    assert_eq!(parsed.rows[1].time - parsed.rows[0].time, 1_800_000);
    let hp60 = parse(
        &fixture("hp60.txt"),
        ActivityDataset::Hp60,
        range,
        "rolling-v3",
    )
    .unwrap();
    assert_eq!(hp60.rows[0].values.len(), 2);
    assert!(
        hp60.rows
            .iter()
            .any(|r| r.values["Hp60"].value.unwrap() > 9.0)
    );
}

#[test]
fn pcn_matches_independent_hourly_reference_without_resampling_output() {
    let range = ActivityRange::parse("2024-05-10", "2024-05-11").unwrap();
    let parsed = parse(
        &fixture("pcn-iaga.txt"),
        ActivityDataset::Pcn,
        range,
        "definitive",
    )
    .unwrap();
    assert_eq!(parsed.rows.len(), 120);
    assert!(
        parsed
            .rows
            .iter()
            .any(|r| r.values["PCN"].value.unwrap() < 0.0)
    );
    let reference: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/activity/pcn-hourly-reference.json"
    ))
    .unwrap();
    for sample in reference["values"].as_array().unwrap() {
        let middle = sample[0].as_i64().unwrap();
        let hour: Vec<_> = parsed
            .rows
            .iter()
            .filter(|r| r.time >= middle - 1_800_000 && r.time < middle + 1_800_000)
            .collect();
        assert_eq!(hour.len(), 60);
        let mean = hour
            .iter()
            .map(|r| r.values["PCN"].value.unwrap())
            .sum::<f64>()
            / 60.0;
        assert!((mean - sample[1].as_f64().unwrap()).abs() <= 0.0051);
    }
    let missing = parse(
        &fixture("pcn-iaga.txt"),
        ActivityDataset::Pcn,
        ActivityRange::parse("2024-01-02", "2024-01-03").unwrap(),
        "definitive",
    )
    .unwrap();
    assert_eq!(missing.rows[0].values["PCN"].raw, Some(99999.0));
    assert_eq!(missing.rows[0].values["PCN"].missing, Some("fill_value"));
}

#[test]
fn pcn_csv_and_half_open_date_ranges_keep_native_identity() {
    let range = ActivityRange::parse("2026-01-01T00:00:00Z", "2026-01-01T00:02:00Z").unwrap();
    let parsed = parse(
        &fixture("pcn-provisional.csv"),
        ActivityDataset::Pcn,
        range,
        "provisional",
    )
    .unwrap();
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows[0].values["PCN"].value, Some(0.06));
    assert_eq!(parsed.rows[0].quality, "provisional");
    assert!(ActivityRange::parse("2024-05-10", "2024-05-10").is_err());
    let days = ActivityRange::parse("2024-02-29", "2024-03-01")
        .unwrap()
        .days();
    assert_eq!(days, [(2024, 2, 29)]);
}

#[test]
fn documented_missing_sentinels_and_invalid_source_times_are_distinct() {
    // Synthetic format-error/missingness boundaries, not scientific samples.
    let original = std::fs::read_to_string(fixture("hp30.txt")).unwrap();
    let temp = tempfile::NamedTempFile::new().unwrap();
    let range = ActivityRange::parse("2024-05-10", "2024-05-11").unwrap();
    std::fs::write(
        temp.path(),
        original.replace(" 1.667    6 ", " -1.000   -1 "),
    )
    .unwrap();
    let parsed = parse(temp.path(), ActivityDataset::Hp30, range, "rolling-v3").unwrap();
    assert_eq!(parsed.rows[0].values["Hp30"].raw, Some(-1.0));
    assert_eq!(parsed.rows[0].values["Hp30"].missing, Some("fill_value"));
    std::fs::write(temp.path(), original.replace("00.0 00.25", "00.0 01.25")).unwrap();
    assert!(parse(temp.path(), ActivityDataset::Hp30, range, "rolling-v3").is_err());
    assert!(
        parse(
            &fixture("pcn-iaga.txt"),
            ActivityDataset::Pcn,
            range,
            "provisional"
        )
        .is_ok()
    );
}
