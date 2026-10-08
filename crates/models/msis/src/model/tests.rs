use ionoray_core::{Epoch, GeodeticPosition};
#[cfg(feature = "nrlmsis21")]
use serde::Deserialize;

use super::*;

#[test]
fn rejects_non_finite_driver() {
    let drivers = MsisDrivers {
        f107a: f64::NAN,
        f107_previous_day: 100.0,
        geomagnetic_activity: MsisGeomagneticActivity::Daily(4.0),
    };
    assert!(matches!(
        validate_drivers(drivers),
        Err(MsisError::InvalidDriver { .. })
    ));
}

#[test]
fn rejects_each_invalid_storm_time_driver() {
    let names = [
        "ap_daily",
        "ap_current",
        "ap_3h_ago",
        "ap_6h_ago",
        "ap_9h_ago",
        "ap_average_12_to_33h",
        "ap_average_36_to_57h",
    ];
    for (index, name) in names.into_iter().enumerate() {
        let mut ap = [1.0; 7];
        ap[index] = -1.0;
        let drivers = MsisDrivers {
            f107a: 100.0,
            f107_previous_day: 100.0,
            geomagnetic_activity: storm_time(ap),
        };
        assert!(matches!(
            validate_drivers(drivers),
            Err(MsisError::InvalidDriver { name: actual, value: -1.0 }) if actual == name
        ));
    }
}

#[cfg(not(feature = "nrlmsis21"))]
#[test]
fn valid_input_reports_disabled_backend() {
    let input = MsisInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, 12, 0, 0, 0).unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
        },
        drivers: MsisDrivers {
            f107a: 70.0,
            f107_previous_day: 68.0,
            geomagnetic_activity: MsisGeomagneticActivity::Daily(4.0),
        },
    };
    assert!(matches!(
        Msis::new(MsisVersion::Nrlmsis21).evaluate(&input),
        Err(MsisError::BackendUnavailable)
    ));
}

#[cfg(feature = "nrlmsis21")]
#[test]
fn matches_official_nrlmsis21_double_precision_reference() {
    assert_reference_case("daily_official_row_74213");
}

#[cfg(feature = "nrlmsis21")]
#[test]
fn matches_unmodified_nrlmsis21_storm_time_reference() {
    assert_reference_case("storm_time_offline_snapshot");
}

#[cfg(feature = "nrlmsis21")]
fn assert_reference_case(id: &str) {
    let reference: ReferenceFile =
        serde_json::from_str(include_str!("../../data/reference.json")).unwrap();
    assert_eq!(
        reference.release_sha256,
        env!("IONORAY_NRLMSIS21_RELEASE_SHA256")
    );
    assert_eq!(
        reference.official_output_sha256,
        "59210a442f175b6b3f9e15034856989eea46d54ddb4fe3057a289a77512b7ce3"
    );
    let case = reference.cases.iter().find(|case| case.id == id).unwrap();
    let [year, month, day, hour, minute, second] = case.utc;
    let [latitude, longitude, altitude] = case.position_deg_km;
    let input = MsisInput {
        query: QueryPoint {
            epoch: Epoch::maybe_from_gregorian_utc(
                year,
                u8::try_from(month).unwrap(),
                u8::try_from(day).unwrap(),
                u8::try_from(hour).unwrap(),
                u8::try_from(minute).unwrap(),
                u8::try_from(second).unwrap(),
                0,
            )
            .unwrap(),
            position: GeodeticPosition::from_degrees_kilometers(latitude, longitude, altitude)
                .unwrap(),
        },
        drivers: MsisDrivers {
            f107a: case.f107a,
            f107_previous_day: case.f107_previous_day,
            geomagnetic_activity: match case.mode {
                ReferenceMode::Daily => MsisGeomagneticActivity::Daily(case.ap[0]),
                ReferenceMode::StormTime => storm_time(case.ap),
            },
        },
    };
    let atmosphere = Msis::new(MsisVersion::Nrlmsis21)
        .evaluate(&input)
        .unwrap()
        .atmosphere;
    assert_relative(
        atmosphere.temperature.get::<kelvin>(),
        case.output.temperature_k,
        5.0e-12,
    );
    assert_relative(
        atmosphere.exospheric_temperature.get::<kelvin>(),
        case.output.exospheric_temperature_k,
        5.0e-12,
    );
    for (actual, expected) in density_values(atmosphere)
        .into_iter()
        .zip(case.output.densities_si)
    {
        assert_relative(actual.unwrap(), expected, 5.0e-12);
    }
}

#[cfg(feature = "nrlmsis21")]
fn assert_relative(actual: f64, expected: f64, tolerance: f64) {
    assert!(((actual - expected) / expected).abs() <= tolerance);
}

fn storm_time(ap: [f64; 7]) -> MsisGeomagneticActivity {
    MsisGeomagneticActivity::StormTime(MsisApHistory {
        daily: ap[0],
        current: ap[1],
        three_hours_ago: ap[2],
        six_hours_ago: ap[3],
        nine_hours_ago: ap[4],
        average_12_to_33_hours: ap[5],
        average_36_to_57_hours: ap[6],
    })
}

#[cfg(feature = "nrlmsis21")]
fn density_values(atmosphere: NeutralAtmosphere) -> [Option<f64>; 10] {
    [
        atmosphere.mass_density_kg_m3,
        atmosphere.n2_number_density_m3,
        atmosphere.o2_number_density_m3,
        atmosphere.o_number_density_m3,
        atmosphere.he_number_density_m3,
        atmosphere.h_number_density_m3,
        atmosphere.ar_number_density_m3,
        atmosphere.n_number_density_m3,
        atmosphere.anomalous_o_number_density_m3,
        atmosphere.no_number_density_m3,
    ]
}

#[cfg(feature = "nrlmsis21")]
#[derive(Deserialize)]
struct ReferenceFile {
    release_sha256: String,
    official_output_sha256: String,
    cases: Vec<ReferenceCase>,
}

#[cfg(feature = "nrlmsis21")]
#[derive(Deserialize)]
struct ReferenceCase {
    id: String,
    mode: ReferenceMode,
    utc: [i32; 6],
    position_deg_km: [f64; 3],
    f107a: f64,
    f107_previous_day: f64,
    ap: [f64; 7],
    output: ReferenceOutput,
}

#[cfg(feature = "nrlmsis21")]
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReferenceMode {
    Daily,
    StormTime,
}

#[cfg(feature = "nrlmsis21")]
#[derive(Deserialize)]
struct ReferenceOutput {
    temperature_k: f64,
    exospheric_temperature_k: f64,
    densities_si: [f64; 10],
}
