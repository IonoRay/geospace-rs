//! S3 contracts exercise production Prepared methods with the pinned offline snapshot.
#![cfg(feature = "standard")]
#[path = "../examples/support/automatic_fixture.rs"]
mod fixture;
#[path = "support/trace.rs"]
mod trace;

#[cfg(test)]
mod tests {
    use super::{fixture, trace::Calls};
    use ionoray_geospace::{
        DataPolicy, Geospace, GeospaceError, HwmRequest, IriDriverOverrides, IriRequest,
        MsisDriverOverrides, MsisRequest, PreparedHwm, PreparedIri, PreparedMsis,
        hwm::{HwmError, HwmGeomagneticActivity},
        iri::IriError,
        msis::{MsisError, MsisGeomagneticActivity},
    };

    async fn baselines() -> (PreparedIri, PreparedHwm, PreparedMsis) {
        let home = tempfile::tempdir().unwrap();
        let gs = Geospace::open(Some(home.path())).await.unwrap();
        let query = fixture::query();
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
        assert!(iri.indices().rz12.is_some() && iri.indices().ig12.is_some());
        assert!(iri.indices().f107_daily.is_some() && iri.indices().f107_81_day.is_some());
        assert!(hwm.ap_index().is_some());
        assert!(msis.indices().f107a.is_some() && msis.indices().f107_previous_day.is_some());
        assert!(msis.indices().ap_daily.is_some());
        assert_eq!(
            msis.indices()
                .ap_three_hourly
                .iter()
                .map(|s| s.value.value())
                .collect::<Vec<_>>(),
            fixture::AP_HISTORY
        );
        drop(gs);
        home.close().unwrap();
        (iri, hwm, msis)
    }

    #[tokio::test]
    async fn scenario_preserves_baseline() {
        let (iri, hwm, msis) = baselines().await;
        let originals = (iri.clone(), hwm.clone(), msis.clone());
        let calls = Calls::default();
        tracing::subscriber::with_default(calls.clone(), || {
            let empty_iri = iri.with_overrides(IriDriverOverrides::default());
            let empty_msis = msis.with_overrides(MsisDriverOverrides::default());
            assert_eq!(empty_iri, iri);
            assert_eq!(empty_msis, msis);
            assert_eq!(empty_iri.evaluate().unwrap(), iri.evaluate().unwrap());
            assert_eq!(empty_msis.evaluate().unwrap(), msis.evaluate().unwrap());
            let changed_iri = iri.with_overrides(IriDriverOverrides {
                ig12: Some(-10.0),
                ..Default::default()
            });
            let changed_hwm =
                hwm.with_activity(HwmGeomagneticActivity::Disturbed { current_ap: 0.0 });
            let changed_msis = msis.with_overrides(MsisDriverOverrides {
                geomagnetic_activity: Some(MsisGeomagneticActivity::Daily(0.0)),
                ..Default::default()
            });
            let result = changed_iri.evaluate().unwrap();
            assert_eq!(&result.input, changed_iri.input());
            assert_eq!(&result.indices, changed_iri.indices());
            let result = changed_hwm.evaluate().unwrap();
            assert_eq!(&result.input, changed_hwm.input());
            assert_eq!(result.ap_index.as_ref(), changed_hwm.ap_index());
            let result = changed_msis.evaluate().unwrap();
            assert_eq!(&result.input, changed_msis.input());
            assert_eq!(&result.indices, changed_msis.indices());
        });
        assert!(calls.reads().is_empty() && calls.preparations().is_empty());
        assert_eq!((iri, hwm, msis), originals);
    }

    #[tokio::test]
    async fn override_clears_only_changed_evidence() {
        let (iri, hwm, msis) = baselines().await;
        let originals = (iri.clone(), hwm.clone(), msis.clone());
        let drivers = iri.input().drivers;
        // Cover each field separately, both equal-value and changed-value overrides.
        for (field, same) in [
            drivers.sunspot_number_12_month,
            drivers.ionospheric_index_12_month,
            drivers.f107_daily,
            drivers.f107_81_day,
        ]
        .into_iter()
        .enumerate()
        {
            for value in [same, same + 1.0] {
                let mut overrides = IriDriverOverrides::default();
                let mut expected_input = *iri.input();
                let mut expected_evidence = iri.indices().clone();
                match field {
                    0 => {
                        overrides.rz12 = Some(value);
                        expected_input.drivers.sunspot_number_12_month = value;
                        expected_evidence.rz12 = None;
                    }
                    1 => {
                        overrides.ig12 = Some(value);
                        expected_input.drivers.ionospheric_index_12_month = value;
                        expected_evidence.ig12 = None;
                    }
                    2 => {
                        overrides.f107_daily = Some(value);
                        expected_input.drivers.f107_daily = value;
                        expected_evidence.f107_daily = None;
                    }
                    _ => {
                        overrides.f107_81_day = Some(value);
                        expected_input.drivers.f107_81_day = value;
                        expected_evidence.f107_81_day = None;
                    }
                }
                let scenario = iri.with_overrides(overrides);
                assert_eq!(scenario.input(), &expected_input);
                assert_eq!(scenario.indices(), &expected_evidence);
            }
        }
        for (field, same) in [
            msis.input().drivers.f107a,
            msis.input().drivers.f107_previous_day,
        ]
        .into_iter()
        .enumerate()
        {
            for value in [same, same + 1.0] {
                let mut overrides = MsisDriverOverrides::default();
                let mut expected_input = *msis.input();
                let mut expected_evidence = msis.indices().clone();
                if field == 0 {
                    overrides.f107a = Some(value);
                    expected_input.drivers.f107a = value;
                    expected_evidence.f107a = None;
                } else {
                    overrides.f107_previous_day = Some(value);
                    expected_input.drivers.f107_previous_day = value;
                    expected_evidence.f107_previous_day = None;
                }
                let scenario = msis.with_overrides(overrides);
                assert_eq!(scenario.input(), &expected_input);
                assert_eq!(scenario.indices(), &expected_evidence);
            }
        }
        for activity in [
            msis.input().drivers.geomagnetic_activity,
            MsisGeomagneticActivity::Daily(0.0),
        ] {
            let scenario = msis.with_overrides(MsisDriverOverrides {
                geomagnetic_activity: Some(activity),
                ..Default::default()
            });
            let mut expected_input = *msis.input();
            expected_input.drivers.geomagnetic_activity = activity;
            let mut expected_evidence = msis.indices().clone();
            expected_evidence.ap_daily = None;
            expected_evidence.ap_three_hourly.clear();
            assert_eq!(scenario.input(), &expected_input);
            assert_eq!(scenario.indices(), &expected_evidence);
        }
        for activity in [
            hwm.input().geomagnetic_activity,
            HwmGeomagneticActivity::Quiet,
        ] {
            let scenario = hwm.with_activity(activity);
            let mut expected_input = *hwm.input();
            expected_input.geomagnetic_activity = activity;
            assert_eq!(scenario.input(), &expected_input);
            assert!(scenario.ap_index().is_none());
        }
        assert_eq!((iri, hwm, msis), originals);
    }

    #[tokio::test]
    async fn activity_switch_is_repeatable() {
        let (iri, hwm, msis) = baselines().await;
        // A -> B -> A and B -> A -> B, with no scientific monotonicity assertion.
        let iri_b = iri.with_overrides(IriDriverOverrides {
            f107_daily: Some(150.0),
            ..Default::default()
        });
        let hwm_b = hwm.with_activity(HwmGeomagneticActivity::Quiet);
        let msis_b = msis.with_overrides(MsisDriverOverrides {
            geomagnetic_activity: Some(MsisGeomagneticActivity::Daily(4.0)),
            ..Default::default()
        });
        for (a, b) in [(&iri, &iri_b), (&iri_b, &iri)] {
            let before = a.evaluate().unwrap();
            b.evaluate().unwrap();
            assert_eq!(before, a.evaluate().unwrap());
        }
        for (a, b) in [(&hwm, &hwm_b), (&hwm_b, &hwm)] {
            let before = a.evaluate().unwrap();
            b.evaluate().unwrap();
            assert_eq!(before, a.evaluate().unwrap());
        }
        for (a, b) in [(&msis, &msis_b), (&msis_b, &msis)] {
            let before = a.evaluate().unwrap();
            b.evaluate().unwrap();
            assert_eq!(before, a.evaluate().unwrap());
        }
    }

    #[tokio::test]
    async fn invalid_scenario_does_not_stop_following_scenarios() {
        let (iri, hwm, msis) = baselines().await;
        let mut statuses = Vec::new();
        for (id, value) in [("a", 70.0), ("invalid", -1.0), ("b", 150.0)] {
            let scenario = iri.with_overrides(IriDriverOverrides {
                f107_daily: Some(value),
                ..Default::default()
            });
            let outcome = scenario.evaluate();
            if id == "invalid" {
                assert!(matches!(
                    &outcome,
                    Err(GeospaceError::Iri(IriError::InvalidDriver { .. }))
                ));
            }
            statuses.push((id, outcome.is_ok()));
        }
        assert_eq!(statuses, [("a", true), ("invalid", false), ("b", true)]);
        statuses.clear();
        for (id, value) in [("a", 0.0), ("invalid", -1.0), ("b", 40.0)] {
            let outcome = hwm
                .with_activity(HwmGeomagneticActivity::Disturbed { current_ap: value })
                .evaluate();
            if id == "invalid" {
                assert!(matches!(
                    &outcome,
                    Err(GeospaceError::Hwm(HwmError::InvalidAp(_)))
                ));
            }
            statuses.push((id, outcome.is_ok()));
        }
        assert_eq!(statuses, [("a", true), ("invalid", false), ("b", true)]);
        statuses.clear();
        for (id, value) in [("a", 0.0), ("invalid", -1.0), ("b", 40.0)] {
            let outcome = msis
                .with_overrides(MsisDriverOverrides {
                    geomagnetic_activity: Some(MsisGeomagneticActivity::Daily(value)),
                    ..Default::default()
                })
                .evaluate();
            if id == "invalid" {
                assert!(matches!(
                    &outcome,
                    Err(GeospaceError::Msis(MsisError::InvalidDriver { .. }))
                ));
            }
            statuses.push((id, outcome.is_ok()));
        }
        assert_eq!(statuses, [("a", true), ("invalid", false), ("b", true)]);
    }
}
