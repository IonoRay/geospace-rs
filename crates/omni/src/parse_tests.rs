use super::*;

#[test]
#[ignore = "requires official May 2024 HRO CDF; set OMNI_TEST_HRO_CDF"]
fn official_hro_matches_independent_ascii() {
    let path = std::env::var_os("OMNI_TEST_HRO_CDF").map_or_else(
        || std::path::PathBuf::from("/tmp/geospace-omni-hro-202405.cdf"),
        std::path::PathBuf::from,
    );
    if !path.try_exists().unwrap() {
        eprintln!("not executed: missing {}", path.display());
        return;
    }
    let cdf = decode(
        &path,
        OmniProduct::Hro,
        Month {
            year: 2024,
            month: 5,
        },
    )
    .unwrap();
    let times = times(&cdf).unwrap();
    for line in include_str!("../tests/fixtures/hro-may2024-reference.asc").lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        let reference_hour: u8 = columns[2].parse().unwrap();
        let reference_minute: u8 = columns[3].parse().unwrap();
        let fields: Vec<f64> = columns.iter().map(|s| s.parse().unwrap()).collect();
        let index = match (fields[1], fields[2], fields[3]) {
            (131.0, 0.0, 0.0) => 12_960,
            (131.0, 18.0, 0.0) => 14_040,
            (132.0, 1.0, 40.0) => 14_500,
            _ => panic!("unexpected reference timestamp"),
        };
        let (_, _, _, hour, minute, ..) =
            epoch(unix_millis(times[index]).unwrap()).to_gregorian_utc();
        assert_eq!(hour, reference_hour);
        assert_eq!(minute, reference_minute);
        // Zero-based columns from NASA's documented high-resolution ASCII format.
        for (name, column, tolerance) in [
            ("F", 13, 0.006),
            ("BX_GSE", 14, 0.006),
            ("BZ_GSM", 18, 0.006),
            ("flow_speed", 21, 0.051),
            ("Vx", 22, 0.051),
            ("proton_density", 25, 0.006),
            ("T", 26, 0.51),
            ("Pressure", 27, 0.006),
        ] {
            let variable = variable(&cdf, name).unwrap();
            let raw = scalar_values(variable).unwrap()[index];
            let actual = sample(&cdf, variable, raw);
            if index == 12_960 {
                assert_eq!(actual.missing, Some("fill_value"));
            } else {
                assert!(
                    (actual.value.unwrap() - fields[column]).abs() <= tolerance,
                    "{name}: {actual:?}, reference {}",
                    fields[column]
                );
            }
        }
    }
}
#[test]
fn cdf_epoch_matches_known_utc() {
    assert_eq!(unix_millis(62_167_219_200_000.0).unwrap(), 0);
    assert_eq!(
        epoch(unix_millis(63_881_740_800_000.0).unwrap()).to_gregorian_utc(),
        (2024, 5, 1, 0, 0, 0, 0)
    );
    assert!(unix_millis(f64::NAN).is_err());
}
#[test]
#[ignore = "requires the official May 2024 HRO2 CDF; set OMNI_TEST_CDF"]
fn official_may_2024() {
    let path = std::env::var_os("OMNI_TEST_CDF").map_or_else(
        || "/tmp/geospace-omni-hro2-202405.cdf".into(),
        std::path::PathBuf::from,
    );
    if !path.try_exists().unwrap() {
        eprintln!("not executed: missing {}", path.display());
        return;
    }
    let cdf = decode(
        &path,
        OmniProduct::Hro2,
        Month {
            year: 2024,
            month: 5,
        },
    )
    .unwrap();
    let times = times(&cdf).unwrap();
    assert_eq!(times.len(), 31 * 24 * 60);
    assert_eq!(unix_millis(times[0]).unwrap(), 1_714_521_600_000);
    assert_eq!(
        unix_millis(*times.last().unwrap()).unwrap(),
        1_717_199_940_000
    );
    assert_eq!(metadata(&cdf).unwrap().len(), VARIABLES.len());
    let speed = variable(&cdf, "flow_speed").unwrap();
    let fill = attribute(&cdf, speed, "FILLVAL")
        .unwrap()
        .first()
        .and_then(numeric)
        .unwrap();
    assert_eq!(sample(&cdf, speed, Some(fill)).missing, Some("fill_value"));
    assert_eq!(
        sample(&cdf, speed, Some(f64::NAN)).missing,
        Some("non_finite")
    );
    let negative = sample(&cdf, speed, Some(-1.0));
    assert_eq!(negative.value, Some(-1.0));
    assert!(negative.outside_valid_range);
}
