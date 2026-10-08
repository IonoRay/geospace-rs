#[cfg(any(feature = "igrf", feature = "iri", feature = "msis"))]
use crate::request::Arguments;
#[cfg(feature = "iri")]
use crate::request::IRI;
#[cfg(feature = "msis")]
use crate::request::MSIS;
use crate::{
    api,
    error::{GeospaceError, translate},
};
#[cfg(feature = "igrf")]
use ionoray_geospace::igrf::{Igrf, IgrfInput, IgrfVersion};
use pyo3::prelude::*;
#[cfg(any(feature = "igrf", feature = "iri", feature = "msis"))]
use pyo3::types::PyDict;

#[test]
#[cfg(feature = "iri")]
fn driver_parsing_preserves_zero_negative_and_none() {
    Python::attach(|py| {
        let dict = py
            .eval(
                pyo3::ffi::c_str!("{'rz12': 0., 'ig12': -1.5, 'f107_daily': None}"),
                None,
                None,
            )
            .unwrap()
            .cast_into::<PyDict>()
            .unwrap();
        let value = Arguments::new(py, Some(&dict), &[IRI])
            .unwrap()
            .iri(false)
            .unwrap();
        assert_eq!(value.rz12, Some(0.));
        assert_eq!(value.ig12, Some(-1.5));
        assert_eq!(value.f107_daily, None);
    });
}
#[test]
#[cfg(feature = "msis")]
fn rejects_unknown_history_key_and_bool() {
    Python::attach(|py| {
        for source in ["{'ap_history': {'unknown': 1}}", "{'f107a': True}"] {
            let source = std::ffi::CString::new(source).unwrap();
            let dict = py
                .eval(&source, None, None)
                .unwrap()
                .cast_into::<PyDict>()
                .unwrap();
            let error = Arguments::new(py, Some(&dict), &[MSIS])
                .unwrap()
                .msis(false)
                .unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        }
    });
}
#[test]
#[cfg(feature = "igrf")]
fn direct_igrf_matches_rust_and_rejects_non_utc() {
    Python::attach(|py| {
        let dict = py.eval(pyo3::ffi::c_str!("dict(at='2020-07-01T12:00:00Z', latitude_deg=30., longitude_deg=120., altitude_km=300.)"), None, None).unwrap().cast_into::<PyDict>().unwrap();
        let result = api::igrf(py, Some(&dict)).unwrap();
        let query = Arguments::new(py, Some(&dict), &[crate::request::POINT])
            .unwrap()
            .query()
            .unwrap();
        let rust = Igrf::new(IgrfVersion::Igrf14)
            .unwrap()
            .evaluate(&IgrfInput { query })
            .unwrap();
        let expected = crate::convert::output(py, &rust).unwrap();
        assert!(result.bind(py).eq(expected.bind(py)).unwrap());
        dict.set_item("at", "2020-07-01T12:00:00").unwrap();
        assert!(
            api::igrf(py, Some(&dict))
                .unwrap_err()
                .is_instance_of::<pyo3::exceptions::PyValueError>(py)
        );
    });
}
#[test]
#[allow(clippy::default_trait_access)] // The report's store summary is a transitive type.
fn typed_errors_have_stable_codes() {
    Python::attach(|py| {
        use ionoray_geospace::GeospaceError as E;
        #[cfg(feature = "iri")]
        use ionoray_geospace::iri::IriError;
        use ionoray_indices::{
            CoverageGap, CoverageGapReason, IndexDataset, IndexField, RangeSyncReport,
            RangeSyncStatus, SourceCheckStatus, SyncMode,
        };
        let gap = CoverageGap {
            field: IndexField::Dst,
            start_utc_ms: 0,
            end_utc_ms: 1,
            reason: CoverageGapReason::OfflineUnverified,
        };
        let mut report = RangeSyncReport {
            dataset: IndexDataset::Dst,
            mode: SyncMode::Offline,
            force: false,
            requested_start_utc_ms: 0,
            requested_end_utc_ms: 1,
            coverage: vec![],
            gaps: vec![gap],
            attempts: vec![],
            coverage_status: RangeSyncStatus::Partial,
            source_check_status: SourceCheckStatus::NotRequested,
            status: RangeSyncStatus::Partial,
            records_imported: 0,
            download_summary: Default::default(),
            changes: ionoray_indices::ChangeSummary::default(),
        };
        let missing = E::PartialIndices(Box::new(report.clone()));
        // Synthetic failed refresh after local coverage became complete.
        report.gaps.clear();
        report.coverage_status = RangeSyncStatus::Complete;
        report.source_check_status = SourceCheckStatus::Failed;
        report.status = RangeSyncStatus::Partial;
        let refresh_failed = E::PartialIndices(Box::new(report));
        for (error, code) in [
            (missing, "data_unavailable"),
            (refresh_failed, "data_refresh_failed"),
            #[cfg(feature = "iri")]
            (E::Iri(IriError::BackendUnavailable), "model_unavailable"),
            #[cfg(feature = "iri")]
            (E::Iri(IriError::BackendPoisoned), "model_failed"),
            #[cfg(feature = "msis")]
            (E::InvalidMsisPreparation("bad"), "internal_error"),
        ] {
            let error = translate(py, error);
            assert!(error.is_instance_of::<GeospaceError>(py));
            assert_eq!(
                error
                    .value(py)
                    .getattr("code")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                code
            );
        }
    });
}

#[test]
fn capabilities_match_build_and_absent_models_fail_explicitly() {
    assert!(api::capabilities().contains(&"indices"));
    assert!(
        api::capabilities()
            .iter()
            .all(|name| ionoray_geospace::capabilities().contains(name))
    );
    for (name, enabled) in [
        ("igrf", cfg!(feature = "igrf")),
        ("iri", cfg!(feature = "iri")),
        ("hwm", cfg!(feature = "hwm")),
        ("msis", cfg!(feature = "msis")),
    ] {
        assert_eq!(api::capabilities().contains(&name), enabled);
    }
    Python::attach(|py| {
        for name in ["igrf", "iri", "hwm", "msis"] {
            if !api::capabilities().contains(&name) {
                let error = match name {
                    "igrf" => api::igrf(py, None),
                    "iri" => api::iri(py, None),
                    "hwm" => api::hwm(py, None),
                    "msis" => api::msis(py, None),
                    _ => unreachable!(),
                }
                .unwrap_err();
                assert_eq!(
                    error
                        .value(py)
                        .getattr("code")
                        .unwrap()
                        .extract::<String>()
                        .unwrap(),
                    "model_unavailable"
                );
            }
        }
    });
}
