//! NRLMSIS-only contracts against the pinned GFZ snapshot and production API.
#![cfg(all(feature = "indices", feature = "msis"))]
#![allow(clippy::float_cmp)]
#[path = "support/trace.rs"]
mod trace;

use ionoray_geospace::{
    DataPolicy, Duration, Epoch, GeodeticPosition, Geospace, GeospaceError, MsisDriverOverrides,
    MsisRequest, QueryPoint,
    msis::{MsisApHistory, MsisGeomagneticActivity},
    units::kelvin,
};
use ionoray_indices::{QualityFlag, ValueDerivation};
use trace::Calls;
use tracing::instrument::WithSubscriber;

const ARCHIVE_SHA256: &str = "a74cd1096e7b7711690ffba819ddf7149bf32f07a787092510407e5aa1742029";

fn query(year: i32, month: u8, day: u8, hour: u8) -> QueryPoint {
    QueryPoint {
        epoch: Epoch::maybe_from_gregorian_utc(year, month, day, hour, 0, 0, 0).unwrap(),
        position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
    }
}

fn explicit_request(query: QueryPoint, activity: MsisGeomagneticActivity) -> MsisRequest {
    MsisRequest {
        query,
        overrides: MsisDriverOverrides {
            f107a: Some(80.0),
            f107_previous_day: Some(75.0),
            geomagnetic_activity: Some(activity),
        },
    }
}

#[tokio::test]
async fn complete_explicit_modes_skip_all_queries_under_every_policy() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let history = MsisApHistory {
        daily: 4.0,
        current: 2.0,
        three_hours_ago: 4.0,
        six_hours_ago: 3.0,
        nine_hours_ago: 4.0,
        average_12_to_33_hours: 3.375,
        average_36_to_57_hours: 2.5,
    };
    for policy in [DataPolicy::Offline, DataPolicy::Ensure, DataPolicy::Refresh] {
        for activity in [
            MsisGeomagneticActivity::Daily(4.0),
            MsisGeomagneticActivity::StormTime(history),
        ] {
            let calls = Calls::default();
            let evaluated = gs
                .evaluate_msis(explicit_request(query(2020, 7, 1, 12), activity), policy)
                .with_subscriber(calls.clone())
                .await
                .unwrap();
            assert_eq!(evaluated.input.drivers.geomagnetic_activity, activity);
            assert!(evaluated.indices.f107a.is_none());
            assert!(evaluated.indices.f107_previous_day.is_none());
            assert!(evaluated.indices.ap_daily.is_none());
            assert_eq!(evaluated.indices.ap_three_hourly.len(), 0);
            assert_eq!(calls.reads().len(), 0);
            assert_eq!(calls.preparations().len(), 0);
        }
    }
    assert_eq!(
        std::fs::read_dir(home.path().join("indices"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn automatic_storm_time_uses_observed_flux_and_exact_ap_windows() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let query = query(2020, 7, 1, 12);
    let prepared = gs
        .prepare_msis(MsisRequest::new(query), DataPolicy::Offline)
        .await
        .unwrap();
    let evidence = prepared.indices();
    let average = evidence.f107a.as_ref().unwrap();
    assert_close(average.value.value(), 69.954_320_987_654_3, 1.0e-12);
    assert_eq!(
        average.derivation,
        ValueDerivation::CenteredMean {
            window_days: 81,
            interpolated_input_count: 0,
        }
    );
    assert_eq!(average.artifact.to_hex(), ARCHIVE_SHA256);
    let previous = evidence.f107_previous_day.as_ref().unwrap();
    assert_eq!(previous.value.value(), 68.1);
    assert_eq!(previous.derivation, ValueDerivation::Source);
    assert_eq!(previous.artifact.to_hex(), ARCHIVE_SHA256);
    assert_eq!(evidence.ap_daily.as_ref().unwrap().value.value(), 4.0);
    assert_eq!(evidence.ap_three_hourly.len(), 20);
    for (index, sample) in evidence.ap_three_hourly.iter().enumerate() {
        assert_eq!(sample.quality, QualityFlag::Provisional);
        assert_eq!(sample.artifact.to_hex(), ARCHIVE_SHA256);
        assert_eq!(
            sample.interval.start,
            query.epoch - Duration::from_hours(f64::from(u32::try_from(index * 3).unwrap()))
        );
        assert_eq!(
            sample.interval.end - sample.interval.start,
            Duration::from_hours(3.0)
        );
    }
    assert_eq!(
        prepared.input().drivers.geomagnetic_activity,
        MsisGeomagneticActivity::StormTime(MsisApHistory {
            daily: 4.0,
            current: 2.0,
            three_hours_ago: 4.0,
            six_hours_ago: 3.0,
            nine_hours_ago: 4.0,
            average_12_to_33_hours: 3.375,
            average_36_to_57_hours: 2.5,
        })
    );

    let original = prepared.clone();
    drop(gs);
    home.close().unwrap();
    let baseline = prepared.evaluate().unwrap();
    assert_close(
        baseline.result.atmosphere.temperature.get::<kelvin>(),
        744.371_773_656_269_4,
        5.0e-12,
    );
    assert_close(
        baseline.result.atmosphere.mass_density_kg_m3.unwrap(),
        5.470_516_997_113_741e-12,
        5.0e-12,
    );
    let scenario = prepared.with_overrides(MsisDriverOverrides {
        f107a: Some(80.0),
        ..Default::default()
    });
    assert_eq!(scenario.input().drivers.f107a, 80.0);
    assert!(scenario.indices().f107a.is_none());
    assert_eq!(
        scenario.indices().f107_previous_day,
        prepared.indices().f107_previous_day
    );
    assert_eq!(scenario.indices().ap_daily, prepared.indices().ap_daily);
    assert_eq!(
        scenario.indices().ap_three_hourly,
        prepared.indices().ap_three_hourly
    );
    assert!(scenario.evaluate().is_ok());
    assert_eq!(prepared, original);
    assert_eq!(prepared.evaluate().unwrap(), baseline);
}

#[tokio::test]
async fn automatic_history_crosses_utc_year_without_losing_bins() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let query = query(2020, 1, 1, 0);
    let prepared = gs
        .prepare_msis(MsisRequest::new(query), DataPolicy::Offline)
        .await
        .unwrap();
    assert_close(
        prepared.indices().f107a.as_ref().unwrap().value.value(),
        71.360_493_827_160_5,
        1.0e-12,
    );
    assert_eq!(
        prepared
            .indices()
            .f107_previous_day
            .as_ref()
            .unwrap()
            .value
            .value(),
        70.5
    );
    let expected = MsisApHistory {
        daily: 2.0,
        current: 2.0,
        three_hours_ago: 3.0,
        six_hours_ago: 5.0,
        nine_hours_ago: 3.0,
        average_12_to_33_hours: 2.5,
        average_36_to_57_hours: 0.875,
    };
    assert_eq!(
        prepared.input().drivers.geomagnetic_activity,
        MsisGeomagneticActivity::StormTime(expected)
    );
    let samples = &prepared.indices().ap_three_hourly;
    assert_eq!(samples[0].interval.start, query.epoch);
    assert_eq!(
        samples[1].interval.start,
        Epoch::maybe_from_gregorian_utc(2019, 12, 31, 21, 0, 0, 0).unwrap()
    );
    assert_eq!(
        samples[19].interval.start,
        Epoch::maybe_from_gregorian_utc(2019, 12, 29, 15, 0, 0, 0).unwrap()
    );
    assert!(prepared.evaluate().is_ok());
}

#[tokio::test]
async fn missing_history_fails_instead_of_guessing_from_daily_ap() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let query = query(1932, 1, 1, 12);
    let daily = explicit_request(query, MsisGeomagneticActivity::Daily(15.0));
    assert!(gs.evaluate_msis(daily, DataPolicy::Offline).await.is_ok());

    let mut missing_history = daily;
    missing_history.overrides.geomagnetic_activity = None;
    assert!(matches!(
        gs.prepare_msis(missing_history, DataPolicy::Offline).await,
        Err(GeospaceError::Indices(_))
    ));
}

fn assert_close(actual: f64, expected: f64, relative_tolerance: f64) {
    assert!(((actual - expected) / expected).abs() <= relative_tolerance);
}
