//! S2 production API contracts using isolated offline homes.
#![cfg(feature = "standard")]
#[path = "../examples/support/automatic_fixture.rs"]
mod fixture;
#[path = "support/trace.rs"]
mod trace;

#[cfg(test)]
mod tests {
    // Exact comparisons verify input passthrough and deterministic recomputation.
    #![allow(clippy::float_cmp)]
    use super::{fixture, trace::Calls};
    use ionoray_geospace::{
        DataPolicy, Duration, Epoch, Geospace, GeospaceError, HwmRequest, IriDriverOverrides,
        IriIndexEvidence, IriRequest, MsisDriverOverrides, MsisIndexEvidence, MsisRequest,
        PreparedHwm, PreparedIri, PreparedMsis, QueryPoint,
        hwm::HwmGeomagneticActivity,
        igrf,
        msis::{MsisApHistory, MsisGeomagneticActivity},
    };
    use tracing::instrument::WithSubscriber;

    fn iri_explicit(query: QueryPoint) -> IriRequest {
        IriRequest {
            query,
            overrides: IriDriverOverrides {
                rz12: Some(5.0),
                ig12: Some(-5.526_666_666_666_667),
                f107_daily: Some(70.0),
                f107_81_day: Some(70.0),
            },
        }
    }
    fn msis_explicit(query: QueryPoint) -> MsisRequest {
        MsisRequest {
            query,
            overrides: MsisDriverOverrides {
                f107a: Some(80.0),
                f107_previous_day: Some(75.0),
                geomagnetic_activity: Some(MsisGeomagneticActivity::Daily(5.0)),
            },
        }
    }

    fn assert_resolved_fields(
        iri: &PreparedIri,
        hwm: &PreparedHwm,
        msis: &PreparedMsis,
        query: QueryPoint,
    ) {
        // Independent raw-source expectations, not recorded model outputs.
        assert!(iri.input().drivers.ionospheric_index_12_month < 0.0);
        assert_eq!(
            iri.input().drivers.ionospheric_index_12_month,
            iri.indices().ig12.as_ref().unwrap().value.value()
        );
        assert_eq!(iri.input().drivers.f107_daily, 71.2);
        assert_eq!(iri.input().drivers.f107_81_day, 72.1);
        assert_eq!(
            hwm.input().geomagnetic_activity,
            HwmGeomagneticActivity::Disturbed { current_ap: 2.0 }
        );
        assert_eq!(msis.input().drivers.f107_previous_day, 68.1);
        let samples = &msis.indices().ap_three_hourly;
        assert_eq!(
            samples.iter().map(|s| s.value.value()).collect::<Vec<_>>(),
            fixture::AP_HISTORY
        );
        for (i, sample) in samples.iter().enumerate() {
            let hours = u32::try_from(i * 3).unwrap();
            assert_eq!(
                sample.interval.start,
                query.epoch - Duration::from_hours(f64::from(hours))
            );
            assert_eq!(
                sample.interval.end - sample.interval.start,
                Duration::from_hours(3.0)
            );
        }
        assert_eq!(
            msis.input().drivers.geomagnetic_activity,
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
    }

    #[tokio::test]
    async fn automatic_matches_prepared() {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let query = fixture::query();
        let calls = Calls::default();
        let (iri, hwm, msis) = async {
            let iri = gs
                .prepare_iri(IriRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap();
            let hwm = gs
                .prepare_hwm(HwmRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap();
            let msis = gs
                .prepare_msis(MsisRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap();
            (iri, hwm, msis)
        }
        .with_subscriber(calls.clone())
        .await;
        let mut expected = vec![
            "iri_rz12",
            "iri_ig12",
            "iri_f107_daily",
            "iri_f107_81_day",
            "ap",
            "f107a",
            "f107",
            "daily_ap",
        ];
        expected.extend(["ap"; 20]);
        assert_eq!(calls.reads(), expected);
        assert_eq!(
            calls.preparations(),
            [
                "[Rz12, Ig12]",
                "[IriF107, IriF107a81]",
                "[Ap3h]",
                "[F107a, F107, DailyAp, Ap3h]"
            ]
        );
        assert_resolved_fields(&iri, &hwm, &msis, query);
        assert_eq!(
            iri.evaluate().unwrap(),
            gs.evaluate_iri(IriRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        );
        assert_eq!(
            hwm.evaluate().unwrap(),
            gs.evaluate_hwm(HwmRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        );
        assert_eq!(
            msis.evaluate().unwrap(),
            gs.evaluate_msis(MsisRequest::new(query), DataPolicy::Offline)
                .await
                .unwrap()
        );
        let repeated = Calls::default();
        async {
            assert_eq!(iri.evaluate().unwrap(), iri.evaluate().unwrap());
            assert_eq!(hwm.evaluate().unwrap(), hwm.evaluate().unwrap());
            assert_eq!(msis.evaluate().unwrap(), msis.evaluate().unwrap());
        }
        .with_subscriber(repeated.clone())
        .await;
        assert_eq!(repeated.reads().len(), 0);
        assert_eq!(repeated.preparations().len(), 0);
        assert!(!home.path().join("indices/wdc-kyoto").exists());
        let igrf = igrf::Igrf::new(igrf::IgrfVersion::Igrf14).unwrap();
        assert!(igrf.evaluate(&igrf::IgrfInput { query }).is_ok());
    }

    #[tokio::test]
    async fn explicit_overrides_skip_lookup() {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let query = fixture::query();
        for policy in [DataPolicy::Offline, DataPolicy::Ensure, DataPolicy::Refresh] {
            let calls = Calls::default();
            async {
                let iri = gs.prepare_iri(iri_explicit(query), policy).await.unwrap();
                let msis = gs.prepare_msis(msis_explicit(query), policy).await.unwrap();
                assert_eq!(*iri.indices(), IriIndexEvidence::default());
                assert_eq!(*msis.indices(), MsisIndexEvidence::default());
                assert_eq!(
                    iri.input().drivers.ionospheric_index_12_month,
                    -5.526_666_666_666_667
                );
                assert!(iri.evaluate().is_ok());
                assert!(msis.evaluate().is_ok());
                for activity in [
                    HwmGeomagneticActivity::Quiet,
                    HwmGeomagneticActivity::Disturbed { current_ap: 0.0 },
                ] {
                    let hwm = gs
                        .prepare_hwm(
                            HwmRequest {
                                query,
                                geomagnetic_activity: Some(activity),
                            },
                            policy,
                        )
                        .await
                        .unwrap();
                    assert!(hwm.ap_index().is_none());
                    assert_eq!(hwm.input().geomagnetic_activity, activity);
                    assert!(hwm.evaluate().is_ok());
                }
            }
            .with_subscriber(calls.clone())
            .await;
            assert_eq!(calls.reads().len(), 0);
            assert_eq!(calls.preparations().len(), 0);
        }
        // Geospace still opens its root explicitly even when no drivers are missing.
        assert!(home.path().is_dir());
        assert_eq!(
            std::fs::read_dir(home.path().join("indices"))
                .unwrap()
                .count(),
            0
        );
        assert!(
            Geospace::open(Some(std::path::Path::new("relative")))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn partial_overrides_read_only_missing_fields() {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let query = fixture::query();
        // All four independent IRI field paths, each with its partner overridden.
        for field in 0..4 {
            let mut request = iri_explicit(query);
            match field {
                0 => request.overrides.rz12 = None,
                1 => request.overrides.ig12 = None,
                2 => request.overrides.f107_daily = None,
                _ => request.overrides.f107_81_day = None,
            }
            let calls = Calls::default();
            let prepared = gs
                .prepare_iri(request, DataPolicy::Offline)
                .with_subscriber(calls.clone())
                .await
                .unwrap();
            assert_eq!(
                calls.reads(),
                [
                    ["iri_rz12"],
                    ["iri_ig12"],
                    ["iri_f107_daily"],
                    ["iri_f107_81_day"]
                ][field]
            );
            assert_eq!(
                calls.preparations(),
                [["[Rz12]"], ["[Ig12]"], ["[IriF107]"], ["[IriF107a81]"]][field]
            );
            let evidence = prepared.indices();
            assert_eq!(
                [
                    evidence.rz12.is_some(),
                    evidence.ig12.is_some(),
                    evidence.f107_daily.is_some(),
                    evidence.f107_81_day.is_some()
                ],
                [field == 0, field == 1, field == 2, field == 3]
            );
            let input = prepared.input().drivers;
            for (actual, override_value) in [
                (input.sunspot_number_12_month, request.overrides.rz12),
                (input.ionospheric_index_12_month, request.overrides.ig12),
                (input.f107_daily, request.overrides.f107_daily),
                (input.f107_81_day, request.overrides.f107_81_day),
            ] {
                if let Some(value) = override_value {
                    assert_eq!(actual, value);
                }
            }
        }
        for field in 0..3 {
            let mut request = msis_explicit(query);
            match field {
                0 => request.overrides.f107a = None,
                1 => request.overrides.f107_previous_day = None,
                _ => request.overrides.geomagnetic_activity = None,
            }
            let calls = Calls::default();
            let prepared = gs
                .prepare_msis(request, DataPolicy::Offline)
                .with_subscriber(calls.clone())
                .await
                .unwrap();
            let expected = match field {
                0 => vec!["f107a"],
                1 => vec!["f107"],
                _ => {
                    let mut v = vec!["daily_ap"];
                    v.extend(["ap"; 20]);
                    v
                }
            };
            assert_eq!(calls.reads(), expected);
            assert_eq!(
                calls.preparations(),
                [["[F107a]"], ["[F107]"], ["[DailyAp, Ap3h]"]][field]
            );
            assert_eq!(prepared.indices().f107a.is_some(), field == 0);
            assert_eq!(prepared.indices().f107_previous_day.is_some(), field == 1);
            assert_eq!(prepared.indices().ap_daily.is_some(), field == 2);
            assert_eq!(
                prepared.indices().ap_three_hourly.len(),
                if field == 2 { 20 } else { 0 }
            );
            let input = prepared.input().drivers;
            if let Some(value) = request.overrides.f107a {
                assert_eq!(input.f107a, value);
            }
            if let Some(value) = request.overrides.f107_previous_day {
                assert_eq!(input.f107_previous_day, value);
            }
            if let Some(value) = request.overrides.geomagnetic_activity {
                assert_eq!(input.geomagnetic_activity, value);
            }
        }
    }

    #[tokio::test]
    async fn msis_requires_complete_history_across_year_boundary() {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let mut query = fixture::query();
        // GFZ starts on 1932-01-01: current ap exists, but the 57h history does not.
        query.epoch = Epoch::maybe_from_gregorian_utc(1932, 1, 1, 12, 0, 0, 0).unwrap();
        let mut request = msis_explicit(query);
        request.overrides.geomagnetic_activity = None;
        assert!(
            gs.prepare_hwm(HwmRequest::new(query), DataPolicy::Offline)
                .await
                .is_ok()
        );
        assert!(matches!(
            gs.prepare_msis(request, DataPolicy::Offline).await,
            Err(GeospaceError::Indices(_))
        ));
    }

    #[tokio::test]
    async fn offline_missing_and_invalid_input_remain_errors() {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let mut query = fixture::query();
        // Outside all bundled source years. No current date or user data involved.
        query.epoch = Epoch::maybe_from_gregorian_utc(1900, 1, 1, 12, 0, 0, 0).unwrap();
        assert!(matches!(
            gs.prepare_iri(IriRequest::new(query), DataPolicy::Offline)
                .await,
            Err(GeospaceError::PartialIndices(_))
        ));
        assert!(matches!(
            gs.prepare_hwm(HwmRequest::new(query), DataPolicy::Offline)
                .await,
            Err(GeospaceError::PartialIndices(_))
        ));
        assert!(matches!(
            gs.prepare_msis(MsisRequest::new(query), DataPolicy::Offline)
                .await,
            Err(GeospaceError::Indices(_))
        ));
        let mut request = iri_explicit(fixture::query());
        request.overrides.f107_daily = Some(-1.0);
        let prepared = gs.prepare_iri(request, DataPolicy::Offline).await.unwrap();
        assert!(matches!(prepared.evaluate(), Err(GeospaceError::Iri(_))));
    }
}
