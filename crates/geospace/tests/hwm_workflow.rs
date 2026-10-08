//! HWM-only contracts against the pinned GFZ snapshot and production API.
#![cfg(all(feature = "indices", feature = "hwm"))]
#![allow(clippy::float_cmp)]
#[path = "support/trace.rs"]
mod trace;

use ionoray_geospace::{
    DataPolicy, Duration, Epoch, GeodeticPosition, Geospace, GeospaceError, HwmRequest, QueryPoint,
    hwm::{Hwm, HwmError, HwmGeomagneticActivity, HwmVersion},
};
use ionoray_indices::QualityFlag;
use trace::Calls;
use tracing::instrument::WithSubscriber;

fn query(hour: u8, minute: u8) -> QueryPoint {
    QueryPoint {
        epoch: Epoch::maybe_from_gregorian_utc(2020, 7, 1, hour, minute, 0, 0).unwrap(),
        position: GeodeticPosition::from_degrees_kilometers(30.0, 120.0, 300.0).unwrap(),
    }
}

#[tokio::test]
async fn quiet_and_explicit_ap_skip_all_queries_under_every_policy() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    for policy in [DataPolicy::Offline, DataPolicy::Ensure, DataPolicy::Refresh] {
        for activity in [
            HwmGeomagneticActivity::Quiet,
            HwmGeomagneticActivity::Disturbed { current_ap: 0.0 },
            HwmGeomagneticActivity::Disturbed { current_ap: 80.0 },
        ] {
            let calls = Calls::default();
            let evaluated = gs
                .evaluate_hwm(
                    HwmRequest {
                        query: query(12, 0),
                        geomagnetic_activity: Some(activity),
                    },
                    policy,
                )
                .with_subscriber(calls.clone())
                .await
                .unwrap();
            assert_eq!(evaluated.input.geomagnetic_activity, activity);
            assert!(evaluated.ap_index.is_none());
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
async fn automatic_ap_uses_half_open_three_hour_bins_and_keeps_source() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    // GFZ snapshot row 32365: 09-12 ap=4, 12-15 ap=2. Not model-derived.
    for (hour, minute, start_hour, expected_ap) in
        [(11, 59, 9, 4.0), (12, 0, 12, 2.0), (13, 17, 12, 2.0)]
    {
        let calls = Calls::default();
        let prepared = gs
            .prepare_hwm(HwmRequest::new(query(hour, minute)), DataPolicy::Offline)
            .with_subscriber(calls.clone())
            .await
            .unwrap();
        assert_eq!(calls.reads(), ["ap"]);
        assert_eq!(calls.preparations(), ["[Ap3h]"]);
        let sample = prepared.ap_index().unwrap();
        assert_eq!(sample.interval.start, query(start_hour, 0).epoch);
        assert_eq!(
            sample.interval.end,
            sample.interval.start + Duration::from_hours(3.0)
        );
        assert_eq!(sample.value.value(), expected_ap);
        assert_eq!(sample.quality, QualityFlag::Provisional);
        assert_ne!(sample.release_id.len(), 0);
        assert_eq!(
            sample.artifact.to_hex(),
            "a74cd1096e7b7711690ffba819ddf7149bf32f07a787092510407e5aa1742029"
        );
        assert_eq!(
            prepared.input().geomagnetic_activity,
            HwmGeomagneticActivity::Disturbed {
                current_ap: expected_ap
            }
        );
        let result = prepared.evaluate().unwrap();
        assert_eq!(result.ap_index.as_ref(), Some(sample));
        assert_eq!(
            result.result,
            Hwm::new(HwmVersion::Hwm14)
                .evaluate(prepared.input())
                .unwrap()
        );
        assert_eq!(
            result,
            gs.evaluate_hwm(HwmRequest::new(query(hour, minute)), DataPolicy::Offline)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn scenarios_survive_store_close_and_preserve_baseline() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let baseline = gs
        .prepare_hwm(HwmRequest::new(query(12, 0)), DataPolicy::Offline)
        .await
        .unwrap();
    let original = baseline.clone();
    drop(gs);
    home.close().unwrap();
    let calls = Calls::default();
    tracing::subscriber::with_default(calls.clone(), || {
        let original_result = baseline.evaluate().unwrap();
        let quiet = baseline.with_activity(HwmGeomagneticActivity::Quiet);
        let quiet_result = quiet.evaluate().unwrap();
        for activity in [
            baseline.input().geomagnetic_activity,
            HwmGeomagneticActivity::Quiet,
            HwmGeomagneticActivity::Disturbed { current_ap: 80.0 },
        ] {
            let scenario = baseline.with_activity(activity);
            assert!(scenario.ap_index().is_none());
            assert_eq!(scenario.input().query, baseline.input().query);
            assert_eq!(scenario.input().geomagnetic_activity, activity);
            let result = scenario.evaluate().unwrap();
            assert!(result.ap_index.is_none());
            assert_eq!(result.result.provenance, original_result.result.provenance);
        }
        for current_ap in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let invalid = baseline.with_activity(HwmGeomagneticActivity::Disturbed { current_ap });
            assert!(matches!(
                invalid.evaluate(),
                Err(GeospaceError::Hwm(HwmError::InvalidAp(_)))
            ));
        }
        assert_eq!(quiet.evaluate().unwrap(), quiet_result);
        assert_eq!(baseline.evaluate().unwrap(), original_result);
        assert_eq!(baseline, original);
    });
    assert!(calls.reads().is_empty() && calls.preparations().is_empty());
}

#[tokio::test]
async fn missing_automatic_ap_is_an_error_but_quiet_still_runs() {
    let home = tempfile::tempdir().unwrap();
    let gs = Geospace::open(Some(home.path())).await.unwrap();
    let mut request = HwmRequest::new(query(12, 0));
    request.query.epoch = Epoch::maybe_from_gregorian_utc(1900, 1, 1, 12, 0, 0, 0).unwrap();
    assert!(matches!(
        gs.prepare_hwm(request, DataPolicy::Offline).await,
        Err(GeospaceError::PartialIndices(_))
    ));
    request.geomagnetic_activity = Some(HwmGeomagneticActivity::Quiet);
    assert!(gs.evaluate_hwm(request, DataPolicy::Offline).await.is_ok());
}
